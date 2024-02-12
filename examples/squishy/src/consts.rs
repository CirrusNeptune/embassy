pub struct HaEndpointConsts {
    pub domain: &'static str,
    pub port: u16,
    pub auth: &'static str,
}

#[cfg(not(feature = "mbp"))]
pub const HA_CONSTS: HaEndpointConsts = HaEndpointConsts {
    domain: "homeassistant.mow",
    port: 80,
    auth: r#"{"type":"auth","access_token":"eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiI4MDliZjQ1YjczOWE0NDMzODEyMTQ5ZmNhZThhZDJjMiIsImlhdCI6MTcwMzE0NDM0NCwiZXhwIjoyMDE4NTA0MzQ0fQ.HQmtuR0i-SH9QKm6gjW60IaA2ANOMA9pg-Kca2X8rjM"}"#,
};

#[cfg(feature = "mbp")]
pub const HA_CONSTS: HaEndpointConsts = HaEndpointConsts {
    domain: "Cirrus-MBP.mow",
    port: 8123,
    auth: r#"{"type":"auth","access_token":"eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiI3NzVhMTU4YmUyYzg0ODdiOGRmY2ZlMmMzNjg2MDVmMyIsImlhdCI6MTcwMzMwMTM5NCwiZXhwIjoyMDE4NjYxMzk0fQ._CVdQEA1reP4SWTb2KpXX9ZCnM2Jt6mZYn4xRGSUeWw"}"#,
};

pub const DESK_STRIP_ENTITY: &str = "light.wiz_rgbww_tunable_726ed4";

pub const ANDROID_TV_ENTITY: &str = "media_player.android_tv_10_0_0_43";
