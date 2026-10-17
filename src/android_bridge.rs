use jni::JNIEnv;
use jni::objects::{JClass, JString};
use jni::sys::jboolean;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::sleep;
use std::time::Duration;

pub static CANCEL_FLAG: AtomicBool = AtomicBool::new(false);

pub const OVERPASS_ENDPOINTS: &[&str] = &[
    "https://overpass-api.de/api/interpreter",
    "https://overpass.kumi.systems/api/interpreter",
    "https://maps.mail.ru/osm/tools/overpass/api/interpreter",
    "https://overpass.private.coffee/api/interpreter",
];

pub fn split_bounding_box(
    min_lat: f64,
    min_lon: f64,
    max_lat: f64,
    max_lon: f64,
    step: f64,
) -> Vec<(f64, f64, f64, f64)> {
    let mut tiles = Vec::new();
    let mut lat = min_lat;
    while lat < max_lat {
        let next_lat = (lat + step).min(max_lat);
        let mut lon = min_lon;
        while lon < max_lon {
            let next_lon = (lon + step).min(max_lon);
            tiles.push((lat, lon, next_lat, next_lon));
            lon = next_lon;
        }
        lat = next_lat;
    }
    tiles
}

pub fn parse_bedrock_version(version_str: &str) -> [i32; 5] {
    let mut result = [0i32; 5];
    let parts: Vec<i32> = version_str
        .trim()
        .split('.')
        .filter_map(|s| s.parse::<i32>().ok())
        .collect();

    for (i, &val) in parts.iter().take(5).enumerate() {
        result[i] = val;
    }
    result
}

pub fn parse_java_data_version(version_str: &str) -> i32 {
    let parts: Vec<i32> = version_str
        .trim()
        .split('.')
        .filter_map(|s| s.parse::<i32>().ok())
        .collect();

    if let Some(&major) = parts.first() {
        if major >= 26 {
            let minor = parts.get(1).copied().unwrap_or(0);
            return 4500 + (minor * 10);
        }
        if major == 1 && parts.get(1).copied().unwrap_or(0) == 21 {
            let patch = parts.get(2).copied().unwrap_or(0);
            return 3953 + patch;
        }
        if major == 1 && parts.get(1).copied().unwrap_or(0) == 20 {
            let patch = parts.get(2).copied().unwrap_or(0);
            return 3463 + patch;
        }
    }
    3953
}

pub fn fetch_overpass_query_with_failover(query: &str) -> Result<String, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| format!("Client build error: {e}"))?;

    for endpoint in OVERPASS_ENDPOINTS {
        for attempt in 1..=3 {
            match client.post(*endpoint).body(query.to_string()).send() {
                Ok(response) => {
                    let status = response.status();
                    if status.is_success() {
                        if let Ok(text) = response.text() {
                            return Ok(text);
                        }
                    } else if status.as_u16() == 429 {
                        sleep(Duration::from_millis(1500 * attempt));
                        continue;
                    }
                }
                Err(_) => {
                    sleep(Duration::from_millis(500 * attempt));
                }
            }
        }
    }
    Err("Все доступные публичные зеркала Overpass API недоступны".to_string())
}

#[no_mangle]
pub extern "system" fn Java_com_arnis_app_NativeBridge_generateWorld(
    mut env: JNIEnv,
    _class: JClass,
    min_lat: f64,
    min_lon: f64,
    max_lat: f64,
    max_lon: f64,
    is_bedrock: jboolean,
    version: JString,
    output_path: JString,
) -> jboolean {
    let version_str: String = match env.get_string(&version) {
        Ok(s) => s.into(),
        Err(_) => return 0,
    };
    let out_path_str: String = match env.get_string(&output_path) {
        Ok(s) => s.into(),
        Err(_) => return 0,
    };

    let is_bedrock_bool = is_bedrock != 0;
    CANCEL_FLAG.store(false, Ordering::SeqCst);

    let out_dir = Path::new(&out_path_str);
    if !out_dir.exists() && std::fs::create_dir_all(out_dir).is_err() {
        return 0;
    }

    if is_bedrock_bool {
        let version_arr = parse_bedrock_version(&version_str);
        let _ = std::fs::write(out_dir.join("levelname.txt"), b"Arnis Android World");
        let _ = std::fs::write(out_dir.join("bedrock_version.txt"), format!("{:?}", version_arr));
    } else {
        let data_version = parse_java_data_version(&version_str);
        let _ = std::fs::write(out_dir.join("data_version.txt"), format!("{}", data_version));
    }

    let tiles = split_bounding_box(min_lat, min_lon, max_lat, max_lon, 0.005);

    for tile in tiles {
        if CANCEL_FLAG.load(Ordering::SeqCst) {
            return 0;
        }

        let query = format!(
            "[out:json][timeout:60];(way[\"building\"]({0},{1},{2},{3});relation[\"building\"]({0},{1},{2},{3});way[\"highway\"]({0},{1},{2},{3});way[\"waterway\"]({0},{1},{2},{3}););out body;>;out skel qt;",
            tile.0, tile.1, tile.2, tile.3
        );

        if fetch_overpass_query_with_failover(&query).is_err() {
            return 0;
        }
    }

    1
}

#[no_mangle]
pub extern "system" fn Java_com_arnis_app_NativeBridge_cancelGeneration(
    _env: JNIEnv,
    _class: JClass,
) {
    CANCEL_FLAG.store(true, Ordering::SeqCst);
}
