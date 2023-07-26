/*-
 * ========================LICENSE_START=================================
 * PREvant REST API
 * %%
 * Copyright (C) 2018 - 2019 aixigo AG
 * %%
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in
 * all copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
 * THE SOFTWARE.
 * =========================LICENSE_END==================================
 */
use super::super::{
    APP_NAME_LABEL, CONTAINER_TYPE_LABEL, IMAGE_LABEL, REPLICATED_ENV_LABEL, SERVICE_NAME_LABEL,
};
use crate::config::ContainerConfig;
use crate::deployment::deployment_unit::{DeployableService, DeploymentStrategy};
use crate::infrastructure::traefik::TraefikMiddleware;
use crate::infrastructure::{TraefikIngressRoute, TraefikRouterRule};
use crate::models::service::Service;
use crate::models::ServiceConfig;
use base64::{engine::general_purpose, Engine};
use chrono::Utc;
use k8s_openapi::api::apps::v1::DeploymentSpec;
use k8s_openapi::api::core::v1::{
    Container, ContainerPort, EnvVar, KeyToPath, LocalObjectReference, PersistentVolumeClaim,
    PersistentVolumeClaimVolumeSource as PVCSource, PodSpec, PodTemplateSpec, ResourceRequirements,
    SecretVolumeSource, Volume, VolumeMount,
};
use k8s_openapi::api::{
    apps::v1::Deployment as V1Deployment, core::v1::Namespace as V1Namespace,
    core::v1::Secret as V1Secret, core::v1::Service as V1Service,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::LabelSelector;
use k8s_openapi::ByteString;
use kube::core::ObjectMeta;
use kube::CustomResource;
use multimap::MultiMap;
use schemars::JsonSchema;
use secstr::SecUtf8;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashSet};
use std::convert::TryFrom;
use std::iter::FromIterator;
use std::path::{Component, PathBuf};
use std::string::ToString;

#[derive(CustomResource, Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[kube(
    group = "traefik.containo.us",
    version = "v1alpha1",
    kind = "IngressRoute",
    namespaced
)]
#[serde(rename_all = "camelCase")]
pub struct IngressRouteSpec {
    pub entrypoints: Option<Vec<String>>,
    pub routes: Option<Vec<TraefikRuleSpec>>,
    pub tls: Option<TraefikTls>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
pub struct TraefikRuleSpec {
    pub kind: String,
    pub r#match: String,
    pub services: Vec<TraefikRuleService>,
    pub middlewares: Option<Vec<TraefikRuleMiddleware>>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
pub struct TraefikRuleService {
    pub kind: Option<String>,
    pub name: String,
    pub port: Option<u16>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
pub struct TraefikRuleMiddleware {
    name: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TraefikTls {
    cert_resolver: Option<String>,
}

#[derive(CustomResource, Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[kube(
    group = "traefik.containo.us",
    version = "v1alpha1",
    kind = "Middleware",
    namespaced
)]
#[serde(rename_all = "camelCase")]
pub struct MiddlewareSpec(Value);

macro_rules! secret_name_from_path {
    ($path:expr) => {{
        $path
            .components()
            .map(|c| match c {
                Component::Normal(c) => c.to_os_string().into_string().unwrap(),
                _ => "".to_string(),
            })
            .filter(|c| !c.is_empty())
            .map(|c| c.replace(".", "-"))
            .collect::<Vec<String>>()
            .join("-")
    }};
}

macro_rules! secret_name_from_name {
    ($path:expr) => {{
        $path
            .file_name()
            .map(|name| name.to_os_string().into_string().unwrap())
            .map(|name| name.replace(".", "-"))
            .unwrap_or_else(String::new)
    }};
}

impl TryFrom<IngressRoute> for TraefikIngressRoute {
    type Error = &'static str;

    fn try_from(value: IngressRoute) -> Result<Self, Self::Error> {
        use std::str::FromStr;

        let k8s_route = value.spec.routes.unwrap().into_iter().next().unwrap();
        let rule = TraefikRouterRule::from_str(&k8s_route.r#match).unwrap();

        Ok(TraefikIngressRoute::with_existing_routing_rules(
            value.spec.entrypoints.unwrap_or_default(),
            rule,
            k8s_route
                .middlewares
                .unwrap_or_default()
                .into_iter()
                .map(|m| m.name)
                .collect(),
            value.spec.tls.unwrap_or_default().cert_resolver,
        ))
    }
}

/// Creates a JSON payload suitable for [Kubernetes' Namespaces](https://kubernetes.io/docs/tasks/administer-cluster/namespaces/)
pub fn namespace_payload(app_name: &String) -> V1Namespace {
    serde_json::from_value(serde_json::json!({
      "apiVersion": "v1",
      "kind": "Namespace",
      "metadata": {
        "name": app_name
      }
    }))
    .expect("Cannot convert value to core/v1/Namespace")
}

/// Creates a JSON payload suitable for [Kubernetes' Deployments](https://kubernetes.io/docs/concepts/workloads/controllers/deployment/)
pub fn deployment_payload(
    app_name: &str,
    service: &DeployableService,
    container_config: &ContainerConfig,
    use_image_pull_secret: bool,
    persistent_volume_claims: &BTreeMap<&str, PersistentVolumeClaim>,
) -> V1Deployment {
    let env = service.env().map(|env| {
        env.iter()
            .map(|env| EnvVar {
                name: env.key().to_string(),
                value: Some(env.value().unsecure().to_string()),
                ..Default::default()
            })
            .collect()
    });

    let annotations = if let Some(replicated_env) = service
        .env()
        .and_then(super::super::replicated_environment_variable_to_json)
    {
        BTreeMap::from([
            (IMAGE_LABEL.to_string(), service.image().to_string()),
            (REPLICATED_ENV_LABEL.to_string(), replicated_env.to_string()),
        ])
    } else {
        BTreeMap::from([(IMAGE_LABEL.to_string(), service.image().to_string())])
    };

    let volume_mounts = match service.files().map(|files| {
        let parent_paths = files
            .iter()
            .filter_map(|(path, _)| path.parent())
            .collect::<HashSet<_>>();

        parent_paths
            .iter()
            .map(|path| VolumeMount {
                name: secret_name_from_path!(path),
                mount_path: path.to_string_lossy().to_string(),
                ..Default::default()
            })
            .collect::<Vec<_>>()
    }) {
        Some(mut val) => {
            for attached_volume in persistent_volume_claims.keys() {
                val.push(VolumeMount {
                    name: format!(
                        "{}-volume",
                        attached_volume
                            .split("/")
                            .last()
                            .unwrap_or_else(|| "default")
                    ),
                    mount_path: format!("/data/{}{}", app_name, attached_volume),
                    ..Default::default()
                });
            }
            Some(val)
        }
        None => {
            let mut val = Vec::new();
            for attached_volume in persistent_volume_claims.keys() {
                val.push(VolumeMount {
                    name: format!(
                        "{}-volume",
                        attached_volume
                            .split("/")
                            .last()
                            .unwrap_or_else(|| "default")
                    ),
                    mount_path: format!("/data/{}{}", app_name, attached_volume),
                    ..Default::default()
                });
            }
            Some(val)
        }
    };

    let volumes = match service.files().map(|files| {
        let files = files
            .iter()
            .filter_map(|(path, _)| path.parent().map(|parent| (parent, path)))
            .collect::<MultiMap<_, _>>();

        files
            .iter_all()
            .map(|(parent, paths)| {
                let items = paths
                    .iter()
                    .map(|path| KeyToPath {
                        key: secret_name_from_name!(path),
                        path: path
                            .file_name()
                            .map_or(String::new(), |name| name.to_string_lossy().to_string()),
                        ..Default::default()
                    })
                    .collect::<Vec<_>>();

                Volume {
                    name: secret_name_from_path!(parent),
                    secret: Some(SecretVolumeSource {
                        secret_name: Some(format!(
                            "{}-{}-secret",
                            app_name,
                            service.service_name()
                        )),
                        items: Some(items),
                        ..Default::default()
                    }),
                    ..Default::default()
                }
            })
            .collect::<Vec<Volume>>()
    }) {
        Some(mut val) => {
            persistent_volume_claims
                .iter()
                .for_each(|(attached_volume, pvc)| {
                    val.push(Volume {
                        name: format!(
                            "{}-volume",
                            attached_volume
                                .split("/")
                                .last()
                                .unwrap_or_else(|| "default")
                        ),
                        persistent_volume_claim: Some(PVCSource {
                            claim_name: pvc.metadata.name.clone().unwrap_or_default(),
                            ..Default::default()
                        }),
                        ..Default::default()
                    });
                });

            Some(val)
        }
        None => {
            let mut val = Vec::new();
            persistent_volume_claims
                .iter()
                .for_each(|(attached_volume, pvc)| {
                    val.push(Volume {
                        name: format!(
                            "{}-volume",
                            attached_volume
                                .split("/")
                                .last()
                                .unwrap_or_else(|| "default")
                        ),
                        persistent_volume_claim: Some(PVCSource {
                            claim_name: pvc.metadata.name.clone().unwrap_or_default(),
                            ..Default::default()
                        }),
                        ..Default::default()
                    });
                });
            Some(val)
        }
    };

    let resources = container_config
        .memory_limit()
        .map(|mem_limit| ResourceRequirements {
            limits: Some(BTreeMap::from([(
                String::from("memory"),
                Quantity(format!("{mem_limit}")),
            )])),
            ..Default::default()
        });

    let labels = BTreeMap::from([
        (APP_NAME_LABEL.to_string(), app_name.to_string()),
        (
            SERVICE_NAME_LABEL.to_string(),
            service.service_name().to_string(),
        ),
        (
            CONTAINER_TYPE_LABEL.to_string(),
            service.container_type().to_string(),
        ),
    ]);

    V1Deployment {
        metadata: ObjectMeta {
            name: Some(format!(
                "{}-{}-deployment",
                app_name,
                service.service_name()
            )),
            namespace: Some(app_name.to_string()),
            labels: Some(labels.clone()),
            annotations: Some(annotations),
            ..Default::default()
        },
        spec: Some(DeploymentSpec {
            replicas: Some(1),
            selector: LabelSelector {
                match_labels: Some(labels.clone()),
                ..Default::default()
            },
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(labels),
                    annotations: Some(deployment_annotations(service)),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    volumes,
                    containers: vec![Container {
                        name: service.service_name().to_string(),
                        image: Some(service.image().to_string()),
                        image_pull_policy: Some(String::from("Always")),
                        env,
                        volume_mounts,
                        ports: Some(vec![ContainerPort {
                            container_port: service.port() as i32,
                            ..Default::default()
                        }]),
                        resources,
                        ..Default::default()
                    }],
                    image_pull_secrets: if use_image_pull_secret {
                        Some(vec![LocalObjectReference {
                            name: Some(format!("{app_name}-image-pull-secret")),
                        }])
                    } else {
                        None
                    },
                    ..Default::default()
                }),
            },
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Creates the value of an [annotations object](https://kubernetes.io/docs/concepts/overview/working-with-objects/annotations/)
/// so that the underlying pod will be deployed according to its [deployment strategy](`DeploymentStrategy`).
///
/// For example, this [popular workaround](https://stackoverflow.com/a/55221174/5088458) will be
/// applied to ensure that a pod will be recreated everytime a deployment with
/// [`DeploymentStrategy::RedeployAlways`] has been initiated.
fn deployment_annotations(service: &DeployableService) -> BTreeMap<String, String> {
    match service.strategy() {
        DeploymentStrategy::RedeployOnImageUpdate(image_id) => {
            BTreeMap::from([(String::from("imageHash"), image_id.clone())])
        }
        DeploymentStrategy::RedeployNever => BTreeMap::new(),
        DeploymentStrategy::RedeployAlways => {
            BTreeMap::from([(String::from("date"), Utc::now().to_rfc3339())])
        }
    }
}

pub fn deployment_replicas_payload(
    app_name: &String,
    service: &Service,
    replicas: u32,
) -> V1Deployment {
    serde_json::from_value(serde_json::json!({
      "apiVersion": "apps/v1",
      "kind": "Deployment",
      "metadata": {
        "name": format!("{}-{}-deployment", app_name, service.service_name()),
        "namespace": app_name,
        "labels": {
          APP_NAME_LABEL: app_name,
          SERVICE_NAME_LABEL: service.service_name(),
          CONTAINER_TYPE_LABEL: service.container_type().to_string()
        }
      },
      "spec": {
        "replicas": replicas,
        "selector": {
          "matchLabels": {
            APP_NAME_LABEL: app_name,
            SERVICE_NAME_LABEL: service.service_name(),
            CONTAINER_TYPE_LABEL: service.container_type().to_string()
          }
        }
      }
    }))
    .expect("Cannot convert value to apps/v1/Deployment")
}

/// Creates a JSON payload suitable for [Kubernetes' Secrets](https://kubernetes.io/docs/concepts/configuration/secret/)
pub fn secrets_payload(
    app_name: &String,
    service_config: &ServiceConfig,
    files: &BTreeMap<PathBuf, SecUtf8>,
) -> V1Secret {
    let secrets = files
        .iter()
        .map(|(path, file_content)| {
            (
                secret_name_from_name!(path),
                Value::String(general_purpose::STANDARD.encode(file_content.unsecure())),
            )
        })
        .collect::<Map<String, Value>>();

    serde_json::from_value(serde_json::json!({
      "apiVersion": "v1",
      "kind": "Secret",
      "metadata": {
        "name": format!("{}-{}-secret", app_name, service_config.service_name()),
        "namespace": app_name,
         APP_NAME_LABEL: app_name,
         SERVICE_NAME_LABEL: service_config.service_name(),
         CONTAINER_TYPE_LABEL: service_config.container_type().to_string()
      },
      "type": "Opaque",
      "data": secrets
    }))
    .expect("Cannot convert value to core/v1/Secret")
}

pub fn image_pull_secret_payload(
    app_name: &str,
    registries_and_credentials: BTreeMap<String, (&str, &SecUtf8)>,
) -> V1Secret {
    use core::iter::FromIterator;
    let data = ByteString(
        serde_json::json!({
            "auths":
            serde_json::Map::from_iter(registries_and_credentials.into_iter().map(
                |(registry, (username, password))| {
                    (
                        registry,
                        serde_json::json!({
                            "username": username.to_string(),
                            "password": password.unsecure().to_string(),
                        }),
                    )
                },
            ))
        })
        .to_string()
        .into_bytes(),
    );

    V1Secret {
        metadata: ObjectMeta {
            name: Some(format!("{app_name}-image-pull-secret")),
            namespace: Some(app_name.to_string()),
            labels: Some(BTreeMap::from([(
                APP_NAME_LABEL.to_string(),
                app_name.to_string(),
            )])),
            ..Default::default()
        },
        immutable: Some(true),
        data: Some(BTreeMap::from([(String::from(".dockerconfigjson"), data)])),
        type_: Some(String::from("kubernetes.io/dockerconfigjson")),
        ..Default::default()
    }
}

/// Creates a JSON payload suitable for [Kubernetes' Services](https://kubernetes.io/docs/concepts/services-networking/service/)
pub fn service_payload(app_name: &String, service_config: &ServiceConfig) -> V1Service {
    serde_json::from_value(serde_json::json!({
      "apiVersion": "v1",
      "kind": "Service",
      "namespace": app_name,
      "metadata": {
        "name": service_config.service_name(),
        APP_NAME_LABEL: app_name,
        SERVICE_NAME_LABEL: service_config.service_name(),
        CONTAINER_TYPE_LABEL: service_config.container_type().to_string()
      },
      "spec": {
        "ports": [
          {
            "name": service_config.service_name(),
            "targetPort": service_config.port(),
            "port": service_config.port()
          }
        ],
        "selector": {
          APP_NAME_LABEL: app_name,
          SERVICE_NAME_LABEL: service_config.service_name(),
          CONTAINER_TYPE_LABEL: service_config.container_type().to_string()
        }
      }
    }))
    .expect("Cannot convert value to core/v1/Service")
}

/// Creates a payload that ensures that Traefik find the correct route in Kubernetes
///
/// See [Traefik Routers](https://docs.traefik.io/v2.0/user-guides/crd-acme/#traefik-routers)
/// for more information.
pub fn ingress_route_payload(app_name: &String, service: &DeployableService) -> IngressRoute {
    let rules = service
        .ingress_route()
        .routes()
        .iter()
        .map(|route| {
            let middlewares = route
                .middlewares()
                .iter()
                .map(|middleware| {
                    let name = match middleware {
                        crate::infrastructure::traefik::TraefikMiddleware::Ref(name) => {
                            name.clone()
                        }
                        crate::infrastructure::traefik::TraefikMiddleware::Spec {
                            name,
                            spec: _,
                        } => name.clone(),
                    };
                    TraefikRuleMiddleware { name }
                })
                .collect::<Vec<_>>();

            TraefikRuleSpec {
                kind: String::from("Rule"),
                r#match: route.rule().to_string(),
                middlewares: Some(middlewares),
                services: vec![TraefikRuleService {
                    kind: Some(String::from("Service")),
                    name: service.service_name().to_string(),
                    port: Some(service.port()),
                }],
            }
        })
        .collect::<Vec<_>>();

    IngressRoute {
        metadata: ObjectMeta {
            name: Some(format!(
                "{}-{}-ingress-route",
                app_name,
                service.service_name()
            )),
            namespace: Some(app_name.to_string()),
            annotations: Some(BTreeMap::from([
                (APP_NAME_LABEL.to_string(), app_name.to_string()),
                (
                    SERVICE_NAME_LABEL.to_string(),
                    service.service_name().to_string(),
                ),
                (
                    CONTAINER_TYPE_LABEL.to_string(),
                    service.container_type().to_string(),
                ),
                (
                    String::from("traefik.ingress.kubernetes.io/router.entrypoints"),
                    String::from("web"),
                ),
            ])),
            ..Default::default()
        },
        spec: IngressRouteSpec {
            routes: Some(rules),
            ..Default::default()
        },
    }
}

/// Creates a payload that ensures that Traefik strips out the path prefix.
///
/// See [Traefik Routers](https://docs.traefik.io/v2.0/user-guides/crd-acme/#traefik-routers)
/// for more information.
pub fn middleware_payload(app_name: &String, service: &DeployableService) -> Vec<Middleware> {
    service
        .ingress_route()
        .routes()
        .iter()
        .flat_map(|r| {
            r.middlewares()
                .iter()
                .filter_map(|middleware| match middleware {
                    TraefikMiddleware::Ref(_) => None,
                    TraefikMiddleware::Spec { name, spec } => Some((name, spec)),
                })
        })
        .map(|(name, spec)| Middleware {
            metadata: ObjectMeta {
                name: Some(name.clone()),
                namespace: Some(app_name.clone()),
                ..Default::default()
            },
            spec: MiddlewareSpec(serde_json::json!(spec)),
        })
        .collect::<Vec<_>>()
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use crate::infrastructure::{TraefikIngressRoute, TraefikRouterRule};
    use crate::models::{AppName, Environment, EnvironmentVariable};
    use crate::sc;

    #[test]
    fn should_create_deployment_payload() {
        let config = sc!("db", "mariadb:10.3.17");

        let payload = deployment_payload(
            "master",
            &DeployableService::new(
                config,
                DeploymentStrategy::RedeployAlways,
                TraefikIngressRoute::with_rule(TraefikRouterRule::path_prefix_rule(&[
                    "master", "db",
                ])),
                Vec::new(),
            ),
            &ContainerConfig::default(),
            false,
            &BTreeMap::new(),
        );

        assert_json_diff::assert_json_include!(
            actual: payload,
            expected: serde_json::json!({
              "apiVersion": "apps/v1",
              "kind": "Deployment",
              "metadata": {
                "annotations": {
                  "com.aixigo.preview.servant.image": "docker.io/library/mariadb:10.3.17"
                },
                "labels": {
                  "com.aixigo.preview.servant.app-name": "master",
                  "com.aixigo.preview.servant.container-type": "instance",
                  "com.aixigo.preview.servant.service-name": "db"
                },
                "name": "master-db-deployment",
                "namespace": "master"
              },
              "spec": {
                "replicas": 1,
                "selector": {
                  "matchLabels": {
                    "com.aixigo.preview.servant.app-name": "master",
                    "com.aixigo.preview.servant.container-type": "instance",
                    "com.aixigo.preview.servant.service-name": "db"
                  }
                },
                "template": {
                  "metadata": {
                    "annotations": {
                    },
                    "labels": {
                      "com.aixigo.preview.servant.app-name": "master",
                      "com.aixigo.preview.servant.container-type": "instance",
                      "com.aixigo.preview.servant.service-name": "db"
                    }
                  },
                  "spec": {
                    "volumes": [{
                      "name": "master-db-storage",
                      "persistentVolumeClaim":{
                        "claimName": "master-db-pvc"
                      }
                    }],
                    "containers": [
                      {
                        "image": "docker.io/library/mariadb:10.3.17",
                        "imagePullPolicy": "Always",
                        "name": "db",
                        "ports": [
                          {
                            "containerPort": 80
                          }
                        ],
                        "volumeMounts": [
                          {
                            "mountPath": "var/lib/data",
                            "name": "data-volume"
                          }
                        ]
                      }
                    ]
                  }
                }
              }
            })
        );
    }

    #[test]
    fn should_create_deployment_with_environment_variable() {
        let mut config = sc!("db", "mariadb:10.3.17");
        config.set_env(Some(Environment::new(vec![EnvironmentVariable::new(
            String::from("MYSQL_ROOT_PASSWORD"),
            SecUtf8::from("example"),
        )])));

        let payload = deployment_payload(
            "master",
            &DeployableService::new(
                config,
                DeploymentStrategy::RedeployAlways,
                TraefikIngressRoute::with_rule(TraefikRouterRule::path_prefix_rule(&[
                    "master", "db",
                ])),
                Vec::new(),
            ),
            &ContainerConfig::default(),
            false,
            &BTreeMap::new(),
        );

        assert_json_diff::assert_json_include!(
            actual: payload,
            expected: serde_json::json!({
              "apiVersion": "apps/v1",
              "kind": "Deployment",
              "metadata": {
                "annotations": {
                  "com.aixigo.preview.servant.image": "docker.io/library/mariadb:10.3.17",
                },
                "labels": {
                  "com.aixigo.preview.servant.app-name": "master",
                  "com.aixigo.preview.servant.container-type": "instance",
                  "com.aixigo.preview.servant.service-name": "db"
                },
                "name": "master-db-deployment",
                "namespace": "master"
              },
              "spec": {
                "replicas": 1,
                "selector": {
                  "matchLabels": {
                    "com.aixigo.preview.servant.app-name": "master",
                    "com.aixigo.preview.servant.container-type": "instance",
                    "com.aixigo.preview.servant.service-name": "db"
                  }
                },
                "template": {
                  "metadata": {
                    "annotations": {
                    },
                    "labels": {
                      "com.aixigo.preview.servant.app-name": "master",
                      "com.aixigo.preview.servant.container-type": "instance",
                      "com.aixigo.preview.servant.service-name": "db"
                    }
                  },
                  "spec": {
                    "containers": [
                      {
                        "env": [],
                        "image": "docker.io/library/mariadb:10.3.17",
                        "imagePullPolicy": "Always",
                        "name": "db",
                        "ports": [
                          {
                            "containerPort": 80
                          }
                        ],
                      }
                    ],
                  }
                }
              }
            })
        );
    }

    #[test]
    fn should_create_deployment_with_replicated_environment_variable() {
        let mut config = sc!("db", "mariadb:10.3.17");
        config.set_env(Some(Environment::new(vec![
            EnvironmentVariable::with_replicated(
                String::from("MYSQL_ROOT_PASSWORD"),
                SecUtf8::from("example"),
            ),
        ])));

        let payload = deployment_payload(
            "master",
            &DeployableService::new(
                config,
                DeploymentStrategy::RedeployAlways,
                TraefikIngressRoute::with_rule(TraefikRouterRule::path_prefix_rule(&[
                    "master", "db",
                ])),
                Vec::new(),
            ),
            &ContainerConfig::default(),
            false,
            &BTreeMap::new(),
        );

        assert_json_diff::assert_json_include!(
            actual: payload,
            expected: serde_json::json!({
              "apiVersion": "apps/v1",
              "kind": "Deployment",
              "metadata": {
                "annotations": {
                  "com.aixigo.preview.servant.image": "docker.io/library/mariadb:10.3.17",
                  "com.aixigo.preview.servant.replicated-env": serde_json::json!({
                      "MYSQL_ROOT_PASSWORD": {
                        "value": "example",
                        "templated": false,
                        "replicate": true,
                      }
                    }).to_string()
                },
                "labels": {
                  "com.aixigo.preview.servant.app-name": "master",
                  "com.aixigo.preview.servant.container-type": "instance",
                  "com.aixigo.preview.servant.service-name": "db"
                },
                "name": "master-db-deployment",
                "namespace": "master"
              },
              "spec": {
                "replicas": 1,
                "selector": {
                  "matchLabels": {
                    "com.aixigo.preview.servant.app-name": "master",
                    "com.aixigo.preview.servant.container-type": "instance",
                    "com.aixigo.preview.servant.service-name": "db"
                  }
                },
                "template": {
                  "metadata": {
                    "annotations": {
                    },
                    "labels": {
                      "com.aixigo.preview.servant.app-name": "master",
                      "com.aixigo.preview.servant.container-type": "instance",
                      "com.aixigo.preview.servant.service-name": "db"
                    }
                  },
                  "spec": {
                    "containers": [
                      {
                        "env": [],
                        "image": "docker.io/library/mariadb:10.3.17",
                        "imagePullPolicy": "Always",
                        "name": "db",
                        "ports": [
                          {
                            "containerPort": 80
                          }
                        ]
                      }
                    ]
                  }
                }
              }
            })
        );
    }

    #[test]
    fn should_create_ingress_route() {
        let app_name = "master".parse::<AppName>().unwrap();
        let mut config = sc!("db", "mariadb:10.3.17");
        let port = 1234;
        config.set_port(port);
        let config = DeployableService::new(
            config,
            DeploymentStrategy::RedeployAlways,
            TraefikIngressRoute::with_defaults(&AppName::from_str("master").unwrap(), "db"),
            Vec::new(),
        );
        let payload = ingress_route_payload(&app_name, &config);

        assert_json_diff::assert_json_include!(
            actual: payload,
            expected: serde_json::json!({
              "apiVersion": "traefik.containo.us/v1alpha1",
              "kind": "IngressRoute",
              "metadata": {
                "name": "master-db-ingress-route",
                "namespace": "master",
              },
              "spec": {
                "routes": [
                  {
                    "match": "PathPrefix(`/master/db/`)",
                    "kind": "Rule",
                    "services": [
                      {
                        "name": "db",
                        "port": port,
                      }
                    ],
                    "middlewares": [
                      {
                        "name": "master-db-middleware",
                      }
                    ]
                  }
                ]
              },
            }),
        );
    }

    #[test]
    fn should_create_middleware_with_default_prefix() {
        let config = sc!("db", "mariadb:10.3.17");
        let service = DeployableService::new(
            config,
            DeploymentStrategy::RedeployAlways,
            TraefikIngressRoute::with_defaults(&AppName::from_str("master").unwrap(), "db"),
            Vec::new(),
        );

        let payload = middleware_payload(&String::from("master"), &service);

        assert_json_diff::assert_json_include!(
            actual: payload,
            expected: serde_json::json!([{
              "apiVersion": "traefik.containo.us/v1alpha1",
              "kind": "Middleware",
              "metadata": {
                "name": "master-db-middleware",
                "namespace": "master",
              },
              "spec": {
                "stripPrefix": {
                  "prefixes": [
                    "/master/db/"
                  ]
                }
              },
            }]),
        );
    }
}
