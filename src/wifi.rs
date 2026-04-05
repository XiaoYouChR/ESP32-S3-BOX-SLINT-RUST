use std::collections::BTreeSet;
use std::convert::TryInto;

use anyhow::{anyhow, Context, Result};
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::modem::Modem;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::wifi::{
    AccessPointInfo, AuthMethod, BlockingWifi, ClientConfiguration, Configuration, EspWifi,
};
use log::info;

#[derive(Clone, Debug)]
pub struct ScannedNetwork {
    pub ssid: String,
    pub auth_method: Option<AuthMethod>,
    pub channel: u8,
    pub rssi: i8,
}

#[derive(Clone, Debug)]
pub struct ConnectedNetwork {
    pub ssid: String,
    pub ip: String,
}

pub struct WifiScanner {
    wifi: BlockingWifi<EspWifi<'static>>,
}

impl WifiScanner {
    pub fn new(modem: Modem<'static>) -> Result<Self> {
        let sys_loop = EspSystemEventLoop::take().context("failed to take system event loop")?;
        let nvs = EspDefaultNvsPartition::take().context("failed to take default NVS partition")?;

        let mut wifi = BlockingWifi::wrap(
            EspWifi::new(modem, sys_loop.clone(), Some(nvs)).context("failed to create wifi driver")?,
            sys_loop,
        )
        .context("failed to wrap wifi driver")?;

        wifi.set_configuration(&Configuration::Client(ClientConfiguration {
            auth_method: AuthMethod::None,
            ..Default::default()
        }))
            .context("failed to set wifi station configuration")?;
        wifi.start().context("failed to start wifi station")?;

        info!("wifi scanner started in station mode");

        Ok(Self { wifi })
    }

    pub fn scan_networks(&mut self) -> Result<Vec<ScannedNetwork>> {
        if !self.wifi.is_started().context("failed to query wifi state")? {
            self.wifi.start().context("failed to restart wifi station")?;
        }

        let mut access_points = self.wifi.scan().context("wifi scan failed")?;
        access_points.sort_by(|left, right| right.signal_strength.cmp(&left.signal_strength));

        let mut seen_ssids = BTreeSet::new();
        let results: Vec<_> = access_points
            .into_iter()
            .filter_map(|ap| normalize_scan_result(ap, &mut seen_ssids))
            .collect();

        info!("wifi scan completed with {} visible networks", results.len());

        Ok(results)
    }

    pub fn connect_network(
        &mut self,
        ssid: &str,
        password: &str,
        auth_method: Option<AuthMethod>,
    ) -> Result<ConnectedNetwork> {
        if !self.wifi.is_started().context("failed to query wifi state")? {
            self.wifi.start().context("failed to restart wifi station")?;
        }

        if self.wifi.is_connected().context("failed to query current wifi connection state")? {
            self.wifi
                .disconnect()
                .context("failed to disconnect current wifi station")?;
        }

        let wifi_configuration = Configuration::Client(ClientConfiguration {
            ssid: ssid
                .try_into()
                .map_err(|_| anyhow!("ssid is too long for Wi-Fi configuration"))?,
            bssid: None,
            auth_method: auth_method_for_connection(auth_method, password),
            password: password
                .try_into()
                .map_err(|_| anyhow!("password is too long for Wi-Fi configuration"))?,
            channel: None,
            ..Default::default()
        });

        info!(
            "connecting to wifi ssid={} auth={}",
            ssid,
            auth_method_label(Some(auth_method_for_connection(auth_method, password)))
        );

        self.wifi
            .set_configuration(&wifi_configuration)
            .context("failed to apply wifi client configuration")?;
        self.wifi
            .connect()
            .with_context(|| format!("failed to connect wifi station to {ssid}"))?;
        self.wifi
            .wait_netif_up()
            .with_context(|| format!("wifi network interface did not come up for {ssid}"))?;

        let ip_info = self
            .wifi
            .wifi()
            .sta_netif()
            .get_ip_info()
            .context("failed to read station ip info")?;

        info!("wifi connected to {ssid} with ip {}", ip_info.ip);

        Ok(ConnectedNetwork {
            ssid: ssid.to_owned(),
            ip: ip_info.ip.to_string(),
        })
    }
}

fn normalize_scan_result(
    ap: AccessPointInfo,
    seen_ssids: &mut BTreeSet<String>,
) -> Option<ScannedNetwork> {
    if ap.ssid.is_empty() {
        return None;
    }

    let ssid = ap.ssid.to_string();
    if !seen_ssids.insert(ssid.clone()) {
        return None;
    }

    Some(ScannedNetwork {
        ssid,
        auth_method: ap.auth_method,
        channel: ap.channel,
        rssi: ap.signal_strength,
    })
}

pub fn auth_method_label(auth_method: Option<AuthMethod>) -> &'static str {
    match auth_method {
        None => "Unknown",
        Some(AuthMethod::None) => "Open",
        Some(AuthMethod::WEP) => "WEP",
        Some(AuthMethod::WPA) => "WPA",
        Some(AuthMethod::WPA2Personal) => "WPA2",
        Some(AuthMethod::WPAWPA2Personal) => "WPA/WPA2",
        Some(AuthMethod::WPA2Enterprise) => "WPA2-Ent",
        Some(AuthMethod::WPA3Personal) => "WPA3",
        Some(AuthMethod::WPA2WPA3Personal) => "WPA2/WPA3",
        Some(AuthMethod::WAPIPersonal) => "WAPI",
    }
}

pub fn auth_method_code(auth_method: Option<AuthMethod>) -> i32 {
    match auth_method {
        None => -1,
        Some(AuthMethod::None) => 0,
        Some(AuthMethod::WEP) => 1,
        Some(AuthMethod::WPA) => 2,
        Some(AuthMethod::WPA2Personal) => 3,
        Some(AuthMethod::WPAWPA2Personal) => 4,
        Some(AuthMethod::WPA2Enterprise) => 5,
        Some(AuthMethod::WPA3Personal) => 6,
        Some(AuthMethod::WPA2WPA3Personal) => 7,
        Some(AuthMethod::WAPIPersonal) => 8,
    }
}

pub fn auth_method_from_code(code: i32) -> Option<AuthMethod> {
    match code {
        -1 => None,
        0 => Some(AuthMethod::None),
        1 => Some(AuthMethod::WEP),
        2 => Some(AuthMethod::WPA),
        3 => Some(AuthMethod::WPA2Personal),
        4 => Some(AuthMethod::WPAWPA2Personal),
        5 => Some(AuthMethod::WPA2Enterprise),
        6 => Some(AuthMethod::WPA3Personal),
        7 => Some(AuthMethod::WPA2WPA3Personal),
        8 => Some(AuthMethod::WAPIPersonal),
        _ => None,
    }
}

pub fn signal_level_from_rssi(rssi: i8) -> i32 {
    match rssi {
        -55..=i8::MAX => 4,
        -67..=-56 => 3,
        -78..=-68 => 2,
        _ => 1,
    }
}

fn auth_method_for_connection(auth_method: Option<AuthMethod>, password: &str) -> AuthMethod {
    match auth_method {
        Some(method) => method,
        None if password.is_empty() => AuthMethod::None,
        None => AuthMethod::WPA2Personal,
    }
}
