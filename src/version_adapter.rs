#[derive(Debug, Clone)]
pub struct TargetVersionConfig {
    pub is_bedrock: bool,
    pub version_str: String,
}

impl TargetVersionConfig {
    pub fn new(is_bedrock: bool, version_str: String) -> Self {
        Self {
            is_bedrock,
            version_str: version_str.trim().to_string(),
        }
    }

    /// Преобразует версию в 5-элементный массив для Bedrock `level.dat`
    /// (поля MinimumCompatibleClientVersion и lastOpenedWithVersion)
    pub fn to_bedrock_version_array(&self) -> [i32; 5] {
        let mut result = [0i32; 5];
        let parts: Vec<i32> = self
            .version_str
            .split('.')
            .filter_map(|s| s.parse::<i32>().ok())
            .collect();

        for (i, &val) in parts.iter().take(5).enumerate() {
            result[i] = val;
        }
        result
    }

    /// Вычисляет NBT `DataVersion` для миров Java Anvil (включая 1.21.11 и ветку 26.xx)
    pub fn to_java_data_version(&self) -> i32 {
        let parts: Vec<i32> = self
            .version_str
            .split('.')
            .filter_map(|s| s.parse::<i32>().ok())
            .collect();

        if let Some(&major) = parts.first() {
            // Поддержка веток 26.xx
            if major >= 26 {
                let minor = parts.get(1).copied().unwrap_or(0);
                return 4500 + (minor * 10);
            }
            // Поддержка 1.21.x и 1.21.11
            if major == 1 && parts.get(1).copied().unwrap_or(0) == 21 {
                let patch = parts.get(2).copied().unwrap_or(0);
                return 3953 + patch;
            }
            // Поддержка 1.20.x
            if major == 1 && parts.get(1).copied().unwrap_or(0) == 20 {
                let patch = parts.get(2).copied().unwrap_or(0);
                return 3463 + patch;
            }
        }

        // Базовый актуальный DataVersion
        3953
    }
}
