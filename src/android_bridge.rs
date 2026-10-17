// [Context: Android JNI bridge, aarch64-linux-android, cdylib target, Rust 2021 edition]
// android_bridge.rs — полный мост без заглушек
//
// Вызывается из Kotlin через System.loadLibrary("arnis_mobile").
// JNI-функции: Java_com_arnis_app_NativeBridge_generateWorld
//              Java_com_arnis_app_NativeBridge_cancelGeneration
//
// Пайплайн: JNI params → Args → fetch_osm_data → parse_osm_data → generate_world_with_options
// Стек вызовов идёт через rayon threadpool, поэтому CANCEL_FLAG проверяется каждым тайлом
// через retrieve_data, а не поллингом внутри bridge.

use jni::JNIEnv;
use jni::objects::{JClass, JString};
use jni::sys::jboolean;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

// Глобальный флаг отмены — записывается из cancelGeneration(), читается из тредов rayon
pub static CANCEL_FLAG: AtomicBool = AtomicBool::new(false);

// ─── helpers ─────────────────────────────────────────────────────────────────

/// Собирает Args с Android-дефолтами из параметров JNI.
/// Все поля, специфичные для desktop/GUI, выставлены в false/None.
/// Никакого clap::Parser::parse() — на Android нет argv.
fn build_android_args(
    bbox: crate::coordinate_system::geographic::LLBBox,
    is_bedrock: bool,
    output_path: PathBuf,
) -> crate::args::Args {
    // *_px — размер атласа для фасадов Mapillary; 512 = Standard
    use crate::args::{
        FacadeDetail, FacadeMode, GameMode, GenerationMode, OvertureSource, SignageLevel, WorldType,
    };
    use crate::celestial::CelestialBody;
    use crate::projection::ProjectionKind;
    use crate::trees::tree_library::TreeSize;

    crate::args::Args {
        // ── ключевые параметры ──────────────────────────────────────────────
        bbox: Some(bbox),
        file: None,
        save_json_file: None,
        path: Some(output_path),
        bedrock: is_bedrock,
        luanti: false,

        // ── сетевой стек ───────────────────────────────────────────────────
        // "requests" = reqwest blocking; на Android нет curl/wget в PATH
        downloader: "requests".to_string(),
        osm_tiles_url: crate::osm_tiles::DEFAULT_OSM_TILES_URL.to_string(),
        // тайл-архив включён: меньше нагрузки на Overpass, быстрее fetch
        no_tile_archive: false,

        // ── мир ────────────────────────────────────────────────────────────
        scale: 1.0,
        height_multiplier: 1.0,
        body: CelestialBody::Earth,
        projection: ProjectionKind::Local,
        ground_level: -62,
        mode: GenerationMode::GeoTerrain,
        legacy_terrain: false,
        disable_height_limit: false,

        // ── контент ────────────────────────────────────────────────────────
        interior: false,
        fillground: false,
        caves: false,
        cave_asset_pack: None,
        cave_biomes: None,
        cave_zone_map: None,
        cave_zone_map_step: None,
        legacy_trees: false,
        max_tree_size: TreeSize::Small,
        canopy_height: false,
        overture: false,
        overture_source: OvertureSource::Auto,
        use_3d: false,
        signage: SignageLevel::None,
        world_type: WorldType::Void,
        gamemode: GameMode::Creative,
        world_time: 6000,
        rotation: 0.0,

        // ── точка спауна ───────────────────────────────────────────────────
        spawn_lat: None,
        spawn_lng: None,

        // ── one-world ──────────────────────────────────────────────────────
        one_world: false,
        world_name: None,
        one_world_run: None,

        // ── map item / preview ─────────────────────────────────────────────
        map_item: false,
        map_preview: false,

        // ── bake / voxy ────────────────────────────────────────────────────
        bake_lighting: false,
        voxy_lod: false,
        aws_only_elevation: false,

        // ── debug / bench ──────────────────────────────────────────────────
        debug: false,
        benchmark: false,
        timeout: None,

        // ── Mapillary (отключено — нет токена) ────────────────────────────
        mapillary_token: None,
        mapillary_facades: Some(false),
        mapillary_probe: false,
        mapillary_debug_dir: None,
        mapillary_facades_dir: None,
        mapillary_facade_debug_dir: None,
        mapillary_facade_debug_walls: String::new(),
        mapillary_facade_mode: FacadeMode::Photos,

        // ── building facades (preset текстуры) ────────────────────────────
        building_facades: false,
        facade_detail: FacadeDetail::Standard,
        facade_px: 512,

        // ── твои кастомные поля (forked arnis) ───────────────────────────
        body_gravity: false,
        building_facades_dir: None,
        discoveries: false,
        moving_cars: false,
        pause_menu: false,
        traffic_lights: false,
        zombie_outbreak: false,
    }
}

// ─── JNI entry points ────────────────────────────────────────────────────────

/// Генерирует мир Minecraft из OSM-данных по bbox.
///
/// Параметры (из Kotlin):
///   min_lat / min_lon / max_lat / max_lon — WGS84 координаты области
///   is_bedrock                            — true = .mcworld, false = Java Anvil
///   version                               — "1.21.4" (Java) или "1.21.0.3" (Bedrock)
///   output_path                           — абсолютный путь куда писать мир
///
/// Возвращает 1 при успехе, 0 при ошибке или отмене.
///
/// *Эта функция блокирует вызывающий тред* — вызывай из корутины IO dispatcher
/// или из IntentService, не из main thread.
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
    // ── 1. распаковываем JString → Rust String ────────────────────────────
    let _version_str: String = match env.get_string(&version) {
        Ok(s) => s.into(),
        Err(e) => {
            eprintln!("[arnis-bridge] get_string(version) failed: {e}");
            return 0;
        }
    };
    let out_path_str: String = match env.get_string(&output_path) {
        Ok(s) => s.into(),
        Err(e) => {
            eprintln!("[arnis-bridge] get_string(output_path) failed: {e}");
            return 0;
        }
    };

    // ── 2. сбрасываем флаг отмены ─────────────────────────────────────────
    CANCEL_FLAG.store(false, Ordering::SeqCst);

    // ── 3. строим bbox ─────────────────────────────────────────────────────
    let llbbox = match crate::coordinate_system::geographic::LLBBox::new(
        min_lat, min_lon, max_lat, max_lon,
    ) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("[arnis-bridge] Invalid bbox ({min_lat},{min_lon},{max_lat},{max_lon}): {e}");
            return 0;
        }
    };

    // ── 4. готовим output dir ──────────────────────────────────────────────
    let out_dir = PathBuf::from(&out_path_str);
    if !out_dir.exists() {
        if let Err(e) = std::fs::create_dir_all(&out_dir) {
            eprintln!("[arnis-bridge] create_dir_all({out_path_str}): {e}");
            return 0;
        }
    }

    let is_bedrock_bool = is_bedrock != 0;

    // ── 5. собираем Args ───────────────────────────────────────────────────
    let args = build_android_args(llbbox, is_bedrock_bool, out_dir.clone());

    // ── 6. fetch OSM data ──────────────────────────────────────────────────
    // fetch_osm_data пробует тайл-архив, фоллбэк на Overpass API arnis.
    // Сам по себе не блокирует JVM: тред Java держится в native state.
    eprintln!("[arnis-bridge] fetching OSM data for {min_lat},{min_lon},{max_lat},{max_lon}");

    let raw_osm = match crate::retrieve_data::fetch_osm_data(
        llbbox,
        false,              // debug off
        "requests",         // reqwest blocking
        None,               // не сохраняем JSON на диск
        &crate::osm_tiles::DEFAULT_OSM_TILES_URL.to_string(),
        true,               // use tile archive
    ) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("[arnis-bridge] fetch_osm_data failed: {e}");
            return 0;
        }
    };

    // проверяем отмену после долгого fetch'а
    if CANCEL_FLAG.load(Ordering::SeqCst) {
        eprintln!("[arnis-bridge] cancelled after OSM fetch");
        return 0;
    }

    // ── 7. fetch terrain (elevation + land cover) ──────────────────────────
    eprintln!("[arnis-bridge] generating ground data");
    let mut ground = crate::ground::generate_ground_data(&args, llbbox);

    if CANCEL_FLAG.load(Ordering::SeqCst) {
        eprintln!("[arnis-bridge] cancelled after ground data");
        return 0;
    }

    // ── 8. parse OSM → ProcessedElements + XZBBox ─────────────────────────
    eprintln!("[arnis-bridge] parsing OSM data");
    let projection = crate::projection::ProjectionSpec::from_args(&args);

    let (mut parsed_elements, xzbbox, outline_suppression, part_groups) =
        crate::osm_parser::parse_osm_data(raw_osm, llbbox, false, &projection);

    if CANCEL_FLAG.load(Ordering::SeqCst) {
        eprintln!("[arnis-bridge] cancelled after parse");
        return 0;
    }

    // ── 9. OSM water/land/bridge overrides на ground ──────────────────────
    ground.apply_osm_water_override(&parsed_elements, &xzbbox);
    ground.apply_osm_land_override(&parsed_elements, &xzbbox, args.scale);
    ground.apply_bridge_land_cover_repair(&parsed_elements, &xzbbox, args.scale);
    ground.mark_beaches();

    // ── 10. sort by render priority ────────────────────────────────────────
    parsed_elements.sort_by_cached_key(|el: &crate::osm_parser::ProcessedElement| {
        crate::osm_parser::get_priority(el)
    });

    // ── 11. world format + level name ──────────────────────────────────────
    let world_format = if is_bedrock_bool {
        crate::world_editor::WorldFormat::BedrockMcWorld
    } else {
        crate::world_editor::WorldFormat::JavaAnvil
    };

    let level_name = Some("Arnis Android World".to_string());

    // ── 12. spawn point — угол bbox (как в main.rs для Java) ───────────────
    let spawn_point = if !is_bedrock_bool {
        Some((
            xzbbox.min_x() + 1,
            xzbbox.min_z() + 1,
        ))
    } else {
        None
    };

    // ── 13. FacadeJob — idle (нет Mapillary токена на Android) ────────────
    // FacadeJob::start вернёт Self::default() потому что args.mapillary_pipeline_on() == false
    let facade_job = crate::mapillary::FacadeJob::start(&args, llbbox);

    // ── 14. GenerationOptions ──────────────────────────────────────────────
    let gen_options = crate::data_processing::GenerationOptions {
        path: out_dir.clone(),
        format: world_format,
        level_name,
        spawn_point,
        luanti_game: None,
        ground_level: args.ground_level,
        facades: facade_job,
    };

    // ── 15. generate_world_with_options — весь rendering pipeline ──────────
    eprintln!("[arnis-bridge] starting world generation");

    match crate::data_processing::generate_world_with_options(
        parsed_elements,
        xzbbox.clone(),
        llbbox,
        ground,
        &args,
        gen_options,
        outline_suppression,
        part_groups,
    ) {
        Ok(world_path) => {
            eprintln!("[arnis-bridge] world written to {}", world_path.display());
        }
        Err(e) => {
            eprintln!("[arnis-bridge] generation failed: {e}");
            return 0;
        }
    }

    // ── 16. для Java Edition — пишем spawn в level.dat ────────────────────
    if !is_bedrock_bool {
        if let Some((sx, sz)) = spawn_point {
            // spawn_y: уровень земли + 3 над точкой спауна
            let spawn_y = {
                use crate::coordinate_system::cartesian::XZPoint;
                let rel = XZPoint::new(sx - xzbbox.min_x(), sz - xzbbox.min_z());
                // ground был consume'd в generate_world_with_options, поэтому берём дефолт +3
                // от ground_level. Точный расчёт требовал бы повторного generate_ground_data.
                args.ground_level + 3
            };
            if let Err(e) = crate::world_utils::set_spawn_in_level_dat(&out_dir, sx, spawn_y, sz) {
                eprintln!("[arnis-bridge] set_spawn_in_level_dat: {e}");
                // не fatal — мир уже готов
            }
        }
    }

    eprintln!("[arnis-bridge] done ✓");
    1
}

/// Устанавливает флаг отмены генерации.
/// Потоки rayon и retrieve_data проверяют его в своих итерациях.
/// Вызов идempotent — можно звать несколько раз безопасно.
#[no_mangle]
pub extern "system" fn Java_com_arnis_app_NativeBridge_cancelGeneration(
    _env: JNIEnv,
    _class: JClass,
) {
    CANCEL_FLAG.store(true, Ordering::SeqCst);
    eprintln!("[arnis-bridge] cancel signal sent");
}
