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
use serde::{de, Deserialize, Deserializer};

#[derive(Clone, Default, Deserialize)]
pub struct ContainerConfig {
    #[serde(deserialize_with = "ContainerConfig::parse_from_memory_string")]
    memory_limit: Option<u64>,
    #[serde(
        default = "ContainerConfig::default_storage",
        deserialize_with = "ContainerConfig::parse_from_storage_string"
    )]
    kubernetes_storage_size: String,
    #[serde(default)]
    kubernetes_storage_enable: bool,
}

impl ContainerConfig {
    fn parse_from_memory_string<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let container_limit = String::deserialize(deserializer)?;

        let (size, unit) = container_limit.split_at(container_limit.len() - 1);
        let limit = size.parse::<u64>().map_err(de::Error::custom)?;

        let exp = match unit.to_lowercase().as_str() {
            "k" => 1,
            "m" => 2,
            "g" => 3,
            _ => 0,
        };

        Ok(Some(limit * 1024_u64.pow(exp)))
    }

    fn parse_from_storage_string<'de, D>(deserializer: D) -> Result<String, D::Error>
    where
        D: Deserializer<'de>,
    {
        let container_limit = String::deserialize(deserializer)?;

        let (size, unit) = container_limit.split_at(container_limit.len() - 1);

        let converted_unit = match unit.to_lowercase().as_str() {
            "k" => "Ki",
            "m" => "Mi",
            "g" => "Gi",
            _ => "",
        };

        Ok(format!("{}{}", size, converted_unit))
    }

    fn default_storage() -> String {
        "1Gi".to_string()
    }

    pub fn memory_limit(&self) -> Option<u64> {
        self.memory_limit
    }

    pub fn kubernetes_storage_size(&self) -> &String {
        &self.kubernetes_storage_size
    }

    pub fn kubernetes_storage_enable(&self) -> bool {
        self.kubernetes_storage_enable
    }
}
