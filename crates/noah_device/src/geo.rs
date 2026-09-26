//! Which country an address belongs to, answered from tables shipped inside
//! noah so the device room never has to ask a service on the internet where
//! the device's traffic is going.

use std::io::Read as _;
use std::net::{IpAddr, Ipv6Addr};
use std::sync::OnceLock;

include!("../data/countries.rs");

const UNKNOWN: u8 = 255;
const IPV4_TABLE: &[u8] = include_bytes!("../data/ipv4-country.bin.gz");
const IPV6_TABLE: &[u8] = include_bytes!("../data/ipv6-country.bin.gz");
/// Land as `#` on a 144 by 72 grid; see data/README.md.
pub const WORLD_DOTS: &str = include_str!("../data/world-dots.txt");
pub const WORLD_DOTS_COLUMNS: usize = 144;
pub const WORLD_DOTS_ROWS: usize = 72;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Country {
    pub code: &'static str,
    pub name: &'static str,
    /// Missing for registry-wide blocks that belong to no one country.
    pub centroid: Option<(f32, f32)>,
}

impl Country {
    fn at(index: u8) -> Option<Self> {
        let (code, name, latitude, longitude) = COUNTRIES.get(usize::from(index))?;
        let centroid = (!latitude.is_nan() && !longitude.is_nan()).then_some((*latitude, *longitude));
        Some(Self {
            code,
            name,
            centroid,
        })
    }
}

/// The country by its two-letter code.
pub fn country_by_code(code: &str) -> Option<Country> {
    COUNTRIES
        .iter()
        .position(|(candidate, _, _, _)| candidate.eq_ignore_ascii_case(code))
        .and_then(|index| u8::try_from(index).ok())
        .and_then(Country::at)
}

/// The country an address was delegated to. Private, loopback and link-local
/// addresses have none.
pub fn country_of(address: IpAddr) -> Option<Country> {
    match address {
        IpAddr::V4(address) => {
            if address.is_private()
                || address.is_loopback()
                || address.is_link_local()
                || address.is_unspecified()
                || address.is_broadcast()
            {
                return None;
            }
            lookup(&tables().ipv4, 4, u128::from(u32::from(address)))
        }
        IpAddr::V6(address) => {
            if let Some(mapped) = address.to_ipv4_mapped() {
                return country_of(IpAddr::V4(mapped));
            }
            if is_local_v6(address) {
                return None;
            }
            lookup(&tables().ipv6, 16, u128::from(address))
        }
    }
}

fn is_local_v6(address: Ipv6Addr) -> bool {
    let first = address.segments()[0];
    address.is_loopback()
        || address.is_unspecified()
        // fe80::/10 link-local, fc00::/7 unique local.
        || (first & 0xffc0) == 0xfe80
        || (first & 0xfe00) == 0xfc00
}

struct Tables {
    ipv4: Vec<u8>,
    ipv6: Vec<u8>,
}

fn tables() -> &'static Tables {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(|| Tables {
        ipv4: inflate(IPV4_TABLE),
        ipv6: inflate(IPV6_TABLE),
    })
}

fn inflate(compressed: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    if let Err(error) = flate2::read::GzDecoder::new(compressed).read_to_end(&mut bytes) {
        log::error!("the address table shipped with noah is unreadable: {error}");
        bytes.clear();
    }
    bytes
}

/// Entries are `width` big-endian address bytes then one country index,
/// sorted by address; an entry covers everything up to the next one.
fn lookup(table: &[u8], width: usize, address: u128) -> Option<Country> {
    let entry_size = width + 1;
    let count = table.len() / entry_size;
    let start_of = |index: usize| -> u128 {
        let offset = index * entry_size;
        table
            .get(offset..offset + width)
            .map(|bytes| bytes.iter().fold(0u128, |acc, byte| (acc << 8) | u128::from(*byte)))
            .unwrap_or(u128::MAX)
    };
    // The last entry whose start is at or below the address.
    let (mut low, mut high) = (0usize, count);
    while low < high {
        let middle = low + (high - low) / 2;
        if start_of(middle) <= address {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    let index = low.checked_sub(1)?;
    let code = *table.get(index * entry_size + width)?;
    if code == UNKNOWN {
        return None;
    }
    Country::at(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn well_known_addresses_land_in_their_countries() {
        let google_dns = country_of("8.8.8.8".parse().unwrap()).unwrap();
        assert_eq!(google_dns.code, "US");
        let cloudflare_v6 = country_of("2606:4700:4700::1111".parse().unwrap()).unwrap();
        assert_eq!(cloudflare_v6.code, "US");
        assert!(country_of("192.168.1.1".parse().unwrap()).is_none());
        assert!(country_of("127.0.0.1".parse().unwrap()).is_none());
        assert!(country_of("fe80::1".parse().unwrap()).is_none());
    }

    #[test]
    fn countries_have_names_and_most_have_centroids() {
        let germany = country_by_code("de").unwrap();
        assert_eq!(germany.name, "Germany");
        assert!(germany.centroid.is_some());
        assert!(country_by_code("zz").is_none());
    }

    #[test]
    fn world_map_has_the_documented_shape() {
        let rows: Vec<&str> = WORLD_DOTS.lines().collect();
        assert_eq!(rows.len(), WORLD_DOTS_ROWS);
        assert!(rows.iter().all(|row| row.len() == WORLD_DOTS_COLUMNS));
    }
}
