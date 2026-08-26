use std::borrow::Cow;

use crate::view::PaletteId;
use crate::world::fields::{EntityKind, FieldDomain, FieldSchema, FieldValueType, StableIdKind};

const NATURAL_FIELD_PREFIX: &str = "field.sekai.core.natural.";

pub(crate) const P4_WATER_ENERGY_BUDGET_LABEL: &str = "P4 水热预算";
pub(crate) const P4_EVAPORATION_MINUS_PRECIPITATION_LABEL: &str = "E-P";
pub(crate) const P4_PLANETARY_ALBEDO_LABEL: &str = "行星反照率";
pub(crate) const P4_GLOBAL_DAILY_MEAN_LABEL: &str = "全球日均";
pub(crate) const P4_GLOBAL_MEAN_LABEL: &str = "全球";
pub(crate) const P4_RELATIVE_CLOSURE_ERROR_LABEL: &str = "相对闭合差";
pub(crate) const P4_TOA_NET_LABEL: &str = "TOA net";
pub(crate) const P4_PRECIPITATION_REFERENCE_LABEL: &str = "降水";
pub(crate) const P4_ASR_LABEL: &str = "ASR";
pub(crate) const P4_OLR_LABEL: &str = "OLR";
pub(crate) const P4_WATER_FLUX_UNIT: &str = "mm/day";
pub(crate) const P4_RADIATIVE_FLUX_UNIT: &str = "W/m²";
pub(crate) const EARTH_REFERENCE_LABEL: &str = "地球参考";

pub(crate) fn localized_field_key(key: &str) -> Cow<'_, str> {
    let Some(tail) = key.strip_prefix(NATURAL_FIELD_PREFIX) else {
        return Cow::Borrowed(key);
    };
    let label = match tail {
        "annual_local_runoff_mm" => "本地年径流量",
        "bedrock_kind" => "基岩类型",
        "circulation_annual_evaporation_mm" => "年蒸发量（环流）",
        "circulation_annual_precipitation_mm" => "年降水量（环流）",
        "circulation_mean_absorbed_shortwave_w_m2" => "年均吸收短波（环流）",
        "circulation_mean_air_temperature_c" => "年均气温（环流）",
        "circulation_mean_outgoing_longwave_w_m2" => "年均出射长波（环流）",
        "circulation_prevailing_wind_m_s" => "盛行风（环流）",
        "circulation_surface_albedo" => "表面反照率（环流）",
        "coastal_deposition_m" => "海岸沉积量",
        "coastal_deposition_rate_m_per_year" => "当前海岸沉积速率",
        "coastal_erosion_m" => "海岸侵蚀量",
        "coastal_erosion_rate_m_per_year" => "当前海岸侵蚀速率",
        "fluvial_erosion_rate_m_per_year" => "当前河流侵蚀速率",
        "hillslope_deposition_m" => "坡面堆积量",
        "hillslope_deposition_rate_m_per_year" => "当前坡面堆积速率",
        "hillslope_erosion_m" => "坡面侵蚀量",
        "hillslope_erosion_rate_m_per_year" => "当前坡面侵蚀速率",
        "isostatic_response_m" => "均衡响应",
        "isostatic_response_rate_m_per_year" => "当前均衡响应速率",
        "primary_elevation_m" => "初级地形高程",
        "routed_sediment_deposition_m" => "河道输沙沉积",
        "routed_sediment_deposition_rate_m_per_year" => "当前河道输沙沉积速率",
        "tectonic_displacement_m" => "构造位移量",
        "tectonic_displacement_rate_m_per_year" => "当前构造位移速率",
        "boundary_kind" => "构造边界类型",
        "boundary_strength" => "构造边界强度",
        "crust_base_elevation_m" => "地壳基准高程",
        "crust_kind" => "地壳类型",
        "crust_thickness_km" => "地壳厚度",
        "drainage_area_km2" => "汇水面积",
        "elevation_m" => "构造地形高程",
        "erosion_resistance" => "抗侵蚀性",
        "fluvial_erosion_depth_m" => "河流侵蚀深度",
        "fracture_intensity" => "裂隙强度",
        "geothermal_potential" => "地热潜力",
        "lake_depth_m" => "湖泊深度",
        "land_ocean" => "海陆分类",
        "latitude_degrees" => "纬度",
        "mantle_heat_flow_mw_m2" => "地幔热流",
        "maritime_influence" => "海洋影响度",
        "mean_annual_discharge_m3_s" => "多年平均流量",
        "metallic_mineral_potential" => "金属矿产潜力",
        "plate_id" => "板块编号",
        "plate_velocity" => "板块速度",
        "preliminary_annual_precipitation_mm" => "初步年降水量",
        "preliminary_mean_air_temperature_c" => "初步年均气温",
        "preliminary_prevailing_wind_m_s" => "初步盛行风",
        "preliminary_temperature_seasonality_c" => "初步气温季节性",
        "regional_offset_m" => "区域起伏",
        "relative_permeability" => "相对渗透率",
        "sediment_deposition_thickness_m" => "沉积厚度",
        "sedimentary_basin_potential" => "沉积盆地潜力",
        "strahler_stream_order" => "斯特拉勒河级",
        "surface_elevation_m" => "当前地表高程",
        "surface_water_kind" => "地表水类型",
        "tectonic_offset_m" => "构造地貌偏移",
        "volcanic_influence" => "火山影响度",
        "volcanic_offset_m" => "火山地貌偏移",
        "crust_kind.oceanic" => "海洋地壳",
        "crust_kind.continental" => "大陆地壳",
        "boundary_kind.none" => "无构造事件",
        "boundary_kind.weak" => "弱边界",
        "boundary_kind.continental_collision" => "大陆碰撞",
        "boundary_kind.subduction" => "俯冲",
        "boundary_kind.continental_rift" => "大陆裂谷",
        "boundary_kind.oceanic_ridge" => "洋中脊",
        "boundary_kind.transform" => "走滑边界",
        "land_ocean.ocean" => "海洋",
        "land_ocean.land" => "陆地",
        "bedrock_kind.oceanic_mafic" => "海洋镁铁质岩",
        "bedrock_kind.continental_crystalline" => "大陆结晶岩",
        "bedrock_kind.sedimentary" => "沉积岩",
        "bedrock_kind.metamorphic" => "变质岩",
        "bedrock_kind.volcanic" => "火山岩",
        "surface_water_kind.dry_land" => "旱地",
        "surface_water_kind.ocean" => "海洋",
        "surface_water_kind.lake" => "湖泊",
        "strahler_stream_order.none" => "无河道",
        _ => return dynamic_or_fallback(key, tail),
    };
    Cow::Borrowed(label)
}

pub(crate) fn localized_field_label(schema: &FieldSchema) -> String {
    let label = schema.display.label_key();
    if label.is_empty() {
        format!(
            "{}.{}@{}",
            schema.id.namespace(),
            schema.id.name(),
            schema.id.version()
        )
    } else {
        localized_field_key(label).into_owned()
    }
}

fn dynamic_or_fallback<'a>(key: &'a str, tail: &str) -> Cow<'a, str> {
    if let Some(number) = tail.strip_prefix("plate_id.plate-") {
        if exact_ascii_digits(number, 2) {
            return Cow::Owned(format!("板块 {number}"));
        }
    }
    if let Some(number) = tail.strip_prefix("strahler_stream_order.order-") {
        if exact_ascii_digits(number, 3) {
            let order = number
                .parse::<u16>()
                .expect("three ASCII digits always fit in u16");
            return Cow::Owned(format!("{order} 级河流"));
        }
    }
    Cow::Borrowed(key)
}

fn exact_ascii_digits(value: &str, expected_len: usize) -> bool {
    value.len() == expected_len && value.bytes().all(|byte| byte.is_ascii_digit())
}

pub(super) const fn localized_domain(domain: FieldDomain) -> &'static str {
    match domain {
        FieldDomain::Global => "全局",
        FieldDomain::Cells => "单元格",
        FieldDomain::Edges => "边",
        FieldDomain::Entities(EntityKind::Species) => "物种实体",
        FieldDomain::Entities(EntityKind::Culture) => "文化实体",
        FieldDomain::Entities(EntityKind::Settlement) => "聚落实体",
        FieldDomain::Entities(EntityKind::Polity) => "政体实体",
    }
}

pub(super) const fn localized_value_type(value_type: FieldValueType) -> &'static str {
    match value_type {
        FieldValueType::ScalarF32 => "标量",
        FieldValueType::CategoryU32 => "分类",
        FieldValueType::Boolean => "布尔值",
        FieldValueType::Vector2F32 => "二维向量",
        FieldValueType::StableIdU32(StableIdKind::Cell) => "单元格标识",
        FieldValueType::StableIdU32(StableIdKind::Edge) => "边标识",
        FieldValueType::StableIdU32(StableIdKind::Species) => "物种标识",
        FieldValueType::StableIdU32(StableIdKind::Culture) => "文化标识",
        FieldValueType::StableIdU32(StableIdKind::Settlement) => "聚落标识",
        FieldValueType::StableIdU32(StableIdKind::Polity) => "政体标识",
    }
}

pub(super) const fn localized_palette(palette: PaletteId) -> &'static str {
    match palette {
        PaletteId::Sequential => "顺序",
        PaletteId::Diverging => "发散",
        PaletteId::Categorical => "分类",
        PaletteId::Hypsometric => "等高地形",
        PaletteId::LandOcean => "海陆",
    }
}
