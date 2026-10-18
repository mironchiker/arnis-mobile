// [Context: Android JNI bridge, aarch64-linux-android, cdylib, Rust 2021]

use jni::JNIEnv;
use jni::objects::{JClass, JString};
use jni::sys::jboolean;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

pub static CANCEL_FLAG: AtomicBool = AtomicBool::new(false);

fn run_generation(
    env: &mut JNIEnv,
    min_lat: f64,
    min_lon: f64,
    max_lat: f64,
    max_lon: f64,
    is_bedrock: jboolean,
    output_path: &JString,
) -> jboolean {
    let out_path_str: String = match env.get_string(output_path) {
        Ok(s) => s.into(),
        Err(e) => { eprintln!("[arnis-bridge] get_string failed: {e}"); return 0; }
    };

    CANCEL_FLAG.store(false, Ordering::SeqCst);

    let llbbox = match crate::coordinate_system::geographic::LLBBox::new(
        min_lat, min_lon, max_lat, max_lon,
    ) {
        Ok(b) => b,
        Err(e) => { eprintln!("[arnis-bridge] invalid bbox: {e}"); return 0; }
    };

    let out_dir = PathBuf::from(&out_path_str);
    if !out_dir.exists() {
        if let Err(e) = std::fs::create_dir_all(&out_dir) {
            eprintln!("[arnis-bridge] create_dir_all failed: {e}"); return 0;
        }
    }

    let is_bedrock_bool = is_bedrock != 0;

    use crate::args::{FacadeDetail, FacadeMode, GameMode, GenerationMode, OvertureSource, SignageLevel, WorldType};
    use crate::celestial::CelestialBody;
    use crate::projection::ProjectionKind;
    use crate::trees::tree_library::TreeSize;

    let args = crate::args::Args {
        bbox: Some(llbbox),
        file: None,
        save_json_file: None,
        path: Some(out_dir.clone()),
        bedrock: is_bedrock_bool,
        luanti: false,
        downloader: "requests".to_string(),
        osm_tiles_url: crate::osm_tiles::DEFAULT_OSM_TILES_URL.to_string(),
        no_tile_archive: false,
        scale: 1.0,
        height_multiplier: 1.0,
        body: CelestialBody::Earth,
        projection: ProjectionKind::Local,
        ground_level: -62,
        mode: GenerationMode::GeoTerrain,
        legacy_terrain: false,
        disable_height_limit: false,
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
        spawn_lat: None,
        spawn_lng: None,
        one_world: false,
        world_name: None,
        one_world_run: None,
        map_item: false,
        map_preview: false,
        bake_lighting: false,
        voxy_lod: false,
        aws_only_elevation: false,
        debug: false,
        benchmark: false,
        timeout: None,
        mapillary_token: None,
        mapillary_facades: Some(false),
        mapillary_probe: false,
        mapillary_debug_dir: None,
        mapillary_facades_dir: None,
        mapillary_facade_debug_dir: None,
        mapillary_facade_debug_walls: String::new(),
        mapillary_facade_mode: FacadeMode::Photos,
        building_facades: false,
        facade_detail: FacadeDetail::Standard,
        facade_px: 512,
        body_gravity: false,
        building_facades_dir: None,
        discoveries: false,
        moving_cars: false,
        pause_menu: false,
        traffic_lights: false,
        zombie_outbreak: false,
    };

    eprintln!("[arnis-bridge] fetching OSM data...");
    let raw_osm = match crate::retrieve_data::fetch_osm_data(
        llbbox, false, "requests", None,
        &crate::osm_tiles::DEFAULT_OSM_TILES_URL.to_string(), true,
    ) {
        Ok(d) => d,
        Err(e) => { eprintln!("[arnis-bridge] fetch_osm_data failed: {e}"); return 0; }
    };

    if CANCEL_FLAG.load(Ordering::SeqCst) { return 0; }

    eprintln!("[arnis-bridge] generating ground...");
    let mut ground = crate::ground::generate_ground_data(&args, llbbox);

    if CANCEL_FLAG.load(Ordering::SeqCst) { return 0; }

    eprintln!("[arnis-bridge] parsing OSM...");
    let projection = crate::projection::ProjectionSpec::from_args(&args);
    let (mut parsed_elements, xzbbox, outline_suppression, part_groups) =
        crate::osm_parser::parse_osm_data(raw_osm, llbbox, false, &projection);

    if CANCEL_FLAG.load(Ordering::SeqCst) { return 0; }

    ground.apply_osm_water_override(&parsed_elements, &xzbbox);
    ground.apply_osm_land_override(&parsed_elements, &xzbbox, args.scale);
    ground.apply_bridge_land_cover_repair(&parsed_elements, &xzbbox, args.scale);
    ground.mark_beaches();

    parsed_elements.sort_by_cached_key(|el: &crate::osm_parser::ProcessedElement| {
        crate::osm_parser::get_priority(el)
    });

    let world_format = if is_bedrock_bool {
        crate::world_editor::WorldFormat::BedrockMcWorld
    } else {
        crate::world_editor::WorldFormat::JavaAnvil
    };

    let spawn_point = if !is_bedrock_bool {
        Some((xzbbox.min_x() + 1, xzbbox.min_z() + 1))
    } else {
        None
    };

    let facade_job = crate::mapillary::FacadeJob::start(&args, llbbox);

    let gen_options = crate::data_processing::GenerationOptions {
        path: out_dir.clone(),
        format: world_format,
        level_name: Some("Arnis Android World".to_string()),
        spawn_point,
        luanti_game: None,
        ground_level: args.ground_level,
        facades: facade_job,
    };

    eprintln!("[arnis-bridge] generating world...");
    match crate::data_processing::generate_world_with_options(
        parsed_elements, xzbbox.clone(), llbbox,
        ground, &args, gen_options,
        outline_suppression, part_groups,
    ) {
        Ok(p) => eprintln!("[arnis-bridge] done: {}", p.display()),
        Err(e) => { eprintln!("[arnis-bridge] generation failed: {e}"); return 0; }
    }

    if !is_bedrock_bool {
        if let Some((sx, sz)) = spawn_point {
            let spawn_y = args.ground_level + 3;
            if let Err(e) = crate::world_utils::set_spawn_in_level_dat(&out_dir, sx, spawn_y, sz) {
                eprintln!("[arnis-bridge] set_spawn failed: {e}");
            }
        }
    }

    eprintln!("[arnis-bridge] ✓");
    1
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
    _version: JString,
    output_path: JString,
) -> jboolean {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_generation(&mut env, min_lat, min_lon, max_lat, max_lon, is_bedrock, &output_path)
    })) {
        Ok(r) => r,
        Err(e) => {
            let msg = e.downcast_ref::<String>().cloned()
                .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "unknown panic".to_string());
            eprintln!("[arnis-bridge] PANIC: {msg}");
            0
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_com_arnis_app_NativeBridge_cancelGeneration(
    _env: JNIEnv,
    _class: JClass,
) {
    CANCEL_FLAG.store(true, Ordering::SeqCst);
    eprintln!("[arnis-bridge] cancelled");
}
