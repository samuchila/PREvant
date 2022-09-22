use std::path::PathBuf;

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
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum Runtime {
    Docker,
    Kubernetes(KubernetesRuntimeConfig),
}

impl Default for Runtime {
    fn default() -> Self {
        Self::Docker
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesRuntimeConfig {
    #[serde(default)]
    downward_api: KubernetesDownwardApiConfig,
}

impl KubernetesRuntimeConfig {
    pub fn downward_api(&self) -> &KubernetesDownwardApiConfig {
        &self.downward_api
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesDownwardApiConfig {
    labels_path: PathBuf,
}

impl KubernetesDownwardApiConfig {
    pub fn labels_path(&self) -> &PathBuf {
        &self.labels_path
    }
}

impl Default for KubernetesDownwardApiConfig {
    fn default() -> Self {
        Self {
            labels_path: PathBuf::from("/run/podinfo/labels"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_from_minimal_config_as_docker_runtime() {
        let runtime_toml = r#"
        type = 'Docker'
        "#;

        let runtime = toml::de::from_str::<Runtime>(runtime_toml).unwrap();

        assert_eq!(runtime, Runtime::Docker);
    }

    #[test]
    fn parse_form_minimal_config_as_kubernetes_runtime() {
        let runtime_toml = r#"
        type = 'Kubernetes'
        "#;

        let runtime = toml::de::from_str::<Runtime>(runtime_toml).unwrap();

        assert_eq!(runtime, Runtime::Kubernetes(Default::default()));
    }

    #[test]
    fn parse_as_kubernetes_runtime_with_label_downward_path() {
        let runtime_toml = r#"
        type = 'Kubernetes'
        [downwardApi]
        labelsPath = '/some/path'
        "#;

        let runtime = toml::de::from_str::<Runtime>(runtime_toml).unwrap();

        assert_eq!(
            runtime,
            Runtime::Kubernetes(KubernetesRuntimeConfig {
                downward_api: KubernetesDownwardApiConfig {
                    labels_path: PathBuf::from("/some/path")
                }
            })
        );
    }

    #[test]
    fn provide_default_labels_path() {
        let runtime_toml = r#"
        type = 'Kubernetes'
        "#;

        let Runtime::Kubernetes(config) = toml::de::from_str::<Runtime>(runtime_toml).unwrap() else { panic!("Need a K8s config") };

        assert_eq!(
            config.downward_api.labels_path(),
            &PathBuf::from("/run/podinfo/labels")
        )
    }
}
