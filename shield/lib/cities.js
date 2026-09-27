// Cities a fake location can stand in, with the time zone and language a
// browser there would have. The tunnel picks by country; you can pick by city.
"use strict";
(() => {
const Shield = (globalThis.Shield = globalThis.Shield || {});

Shield.CITIES = [
  { id: "new-york", name: "New York", country: "US", lat: 40.7128, lon: -74.006, tz: "America/New_York", locale: "en-US" },
  { id: "los-angeles", name: "Los Angeles", country: "US", lat: 34.0522, lon: -118.2437, tz: "America/Los_Angeles", locale: "en-US" },
  { id: "chicago", name: "Chicago", country: "US", lat: 41.8781, lon: -87.6298, tz: "America/Chicago", locale: "en-US" },
  { id: "miami", name: "Miami", country: "US", lat: 25.7617, lon: -80.1918, tz: "America/New_York", locale: "en-US" },
  { id: "seattle", name: "Seattle", country: "US", lat: 47.6062, lon: -122.3321, tz: "America/Los_Angeles", locale: "en-US" },
  { id: "toronto", name: "Toronto", country: "CA", lat: 43.6532, lon: -79.3832, tz: "America/Toronto", locale: "en-CA" },
  { id: "vancouver", name: "Vancouver", country: "CA", lat: 49.2827, lon: -123.1207, tz: "America/Vancouver", locale: "en-CA" },
  { id: "mexico-city", name: "Mexico City", country: "MX", lat: 19.4326, lon: -99.1332, tz: "America/Mexico_City", locale: "es-MX" },
  { id: "sao-paulo", name: "São Paulo", country: "BR", lat: -23.5505, lon: -46.6333, tz: "America/Sao_Paulo", locale: "pt-BR" },
  { id: "buenos-aires", name: "Buenos Aires", country: "AR", lat: -34.6037, lon: -58.3816, tz: "America/Argentina/Buenos_Aires", locale: "es-AR" },
  { id: "bogota", name: "Bogotá", country: "CO", lat: 4.711, lon: -74.0721, tz: "America/Bogota", locale: "es-CO" },
  { id: "santiago", name: "Santiago", country: "CL", lat: -33.4489, lon: -70.6693, tz: "America/Santiago", locale: "es-CL" },
  { id: "london", name: "London", country: "GB", lat: 51.5074, lon: -0.1278, tz: "Europe/London", locale: "en-GB" },
  { id: "manchester", name: "Manchester", country: "GB", lat: 53.4808, lon: -2.2426, tz: "Europe/London", locale: "en-GB" },
  { id: "dublin", name: "Dublin", country: "IE", lat: 53.3498, lon: -6.2603, tz: "Europe/Dublin", locale: "en-IE" },
  { id: "paris", name: "Paris", country: "FR", lat: 48.8566, lon: 2.3522, tz: "Europe/Paris", locale: "fr-FR" },
  { id: "berlin", name: "Berlin", country: "DE", lat: 52.52, lon: 13.405, tz: "Europe/Berlin", locale: "de-DE" },
  { id: "frankfurt", name: "Frankfurt", country: "DE", lat: 50.1109, lon: 8.6821, tz: "Europe/Berlin", locale: "de-DE" },
  { id: "munich", name: "Munich", country: "DE", lat: 48.1351, lon: 11.582, tz: "Europe/Berlin", locale: "de-DE" },
  { id: "amsterdam", name: "Amsterdam", country: "NL", lat: 52.3676, lon: 4.9041, tz: "Europe/Amsterdam", locale: "nl-NL" },
  { id: "brussels", name: "Brussels", country: "BE", lat: 50.8503, lon: 4.3517, tz: "Europe/Brussels", locale: "nl-BE" },
  { id: "zurich", name: "Zurich", country: "CH", lat: 47.3769, lon: 8.5417, tz: "Europe/Zurich", locale: "de-CH" },
  { id: "vienna", name: "Vienna", country: "AT", lat: 48.2082, lon: 16.3738, tz: "Europe/Vienna", locale: "de-AT" },
  { id: "madrid", name: "Madrid", country: "ES", lat: 40.4168, lon: -3.7038, tz: "Europe/Madrid", locale: "es-ES" },
  { id: "barcelona", name: "Barcelona", country: "ES", lat: 41.3874, lon: 2.1686, tz: "Europe/Madrid", locale: "es-ES" },
  { id: "lisbon", name: "Lisbon", country: "PT", lat: 38.7223, lon: -9.1393, tz: "Europe/Lisbon", locale: "pt-PT" },
  { id: "rome", name: "Rome", country: "IT", lat: 41.9028, lon: 12.4964, tz: "Europe/Rome", locale: "it-IT" },
  { id: "milan", name: "Milan", country: "IT", lat: 45.4642, lon: 9.19, tz: "Europe/Rome", locale: "it-IT" },
  { id: "stockholm", name: "Stockholm", country: "SE", lat: 59.3293, lon: 18.0686, tz: "Europe/Stockholm", locale: "sv-SE" },
  { id: "oslo", name: "Oslo", country: "NO", lat: 59.9139, lon: 10.7522, tz: "Europe/Oslo", locale: "nb-NO" },
  { id: "copenhagen", name: "Copenhagen", country: "DK", lat: 55.6761, lon: 12.5683, tz: "Europe/Copenhagen", locale: "da-DK" },
  { id: "helsinki", name: "Helsinki", country: "FI", lat: 60.1699, lon: 24.9384, tz: "Europe/Helsinki", locale: "fi-FI" },
  { id: "warsaw", name: "Warsaw", country: "PL", lat: 52.2297, lon: 21.0122, tz: "Europe/Warsaw", locale: "pl-PL" },
  { id: "prague", name: "Prague", country: "CZ", lat: 50.0755, lon: 14.4378, tz: "Europe/Prague", locale: "cs-CZ" },
  { id: "budapest", name: "Budapest", country: "HU", lat: 47.4979, lon: 19.0402, tz: "Europe/Budapest", locale: "hu-HU" },
  { id: "bucharest", name: "Bucharest", country: "RO", lat: 44.4268, lon: 26.1025, tz: "Europe/Bucharest", locale: "ro-RO" },
  { id: "athens", name: "Athens", country: "GR", lat: 37.9838, lon: 23.7275, tz: "Europe/Athens", locale: "el-GR" },
  { id: "istanbul", name: "Istanbul", country: "TR", lat: 41.0082, lon: 28.9784, tz: "Europe/Istanbul", locale: "tr-TR" },
  { id: "kyiv", name: "Kyiv", country: "UA", lat: 50.4501, lon: 30.5234, tz: "Europe/Kyiv", locale: "uk-UA" },
  { id: "moscow", name: "Moscow", country: "RU", lat: 55.7558, lon: 37.6173, tz: "Europe/Moscow", locale: "ru-RU" },
  { id: "tel-aviv", name: "Tel Aviv", country: "IL", lat: 32.0853, lon: 34.7818, tz: "Asia/Jerusalem", locale: "he-IL" },
  { id: "dubai", name: "Dubai", country: "AE", lat: 25.2048, lon: 55.2708, tz: "Asia/Dubai", locale: "ar-AE" },
  { id: "riyadh", name: "Riyadh", country: "SA", lat: 24.7136, lon: 46.6753, tz: "Asia/Riyadh", locale: "ar-SA" },
  { id: "cairo", name: "Cairo", country: "EG", lat: 30.0444, lon: 31.2357, tz: "Africa/Cairo", locale: "ar-EG" },
  { id: "lagos", name: "Lagos", country: "NG", lat: 6.5244, lon: 3.3792, tz: "Africa/Lagos", locale: "en-NG" },
  { id: "nairobi", name: "Nairobi", country: "KE", lat: -1.2921, lon: 36.8219, tz: "Africa/Nairobi", locale: "en-KE" },
  { id: "johannesburg", name: "Johannesburg", country: "ZA", lat: -26.2041, lon: 28.0473, tz: "Africa/Johannesburg", locale: "en-ZA" },
  { id: "mumbai", name: "Mumbai", country: "IN", lat: 19.076, lon: 72.8777, tz: "Asia/Kolkata", locale: "en-IN" },
  { id: "delhi", name: "Delhi", country: "IN", lat: 28.6139, lon: 77.209, tz: "Asia/Kolkata", locale: "hi-IN" },
  { id: "bangalore", name: "Bengaluru", country: "IN", lat: 12.9716, lon: 77.5946, tz: "Asia/Kolkata", locale: "en-IN" },
  { id: "karachi", name: "Karachi", country: "PK", lat: 24.8607, lon: 67.0011, tz: "Asia/Karachi", locale: "ur-PK" },
  { id: "dhaka", name: "Dhaka", country: "BD", lat: 23.8103, lon: 90.4125, tz: "Asia/Dhaka", locale: "bn-BD" },
  { id: "bangkok", name: "Bangkok", country: "TH", lat: 13.7563, lon: 100.5018, tz: "Asia/Bangkok", locale: "th-TH" },
  { id: "ho-chi-minh", name: "Ho Chi Minh City", country: "VN", lat: 10.8231, lon: 106.6297, tz: "Asia/Ho_Chi_Minh", locale: "vi-VN" },
  { id: "jakarta", name: "Jakarta", country: "ID", lat: -6.2088, lon: 106.8456, tz: "Asia/Jakarta", locale: "id-ID" },
  { id: "kuala-lumpur", name: "Kuala Lumpur", country: "MY", lat: 3.139, lon: 101.6869, tz: "Asia/Kuala_Lumpur", locale: "ms-MY" },
  { id: "singapore", name: "Singapore", country: "SG", lat: 1.3521, lon: 103.8198, tz: "Asia/Singapore", locale: "en-SG" },
  { id: "manila", name: "Manila", country: "PH", lat: 14.5995, lon: 120.9842, tz: "Asia/Manila", locale: "en-PH" },
  { id: "hong-kong", name: "Hong Kong", country: "HK", lat: 22.3193, lon: 114.1694, tz: "Asia/Hong_Kong", locale: "zh-HK" },
  { id: "taipei", name: "Taipei", country: "TW", lat: 25.033, lon: 121.5654, tz: "Asia/Taipei", locale: "zh-TW" },
  { id: "shanghai", name: "Shanghai", country: "CN", lat: 31.2304, lon: 121.4737, tz: "Asia/Shanghai", locale: "zh-CN" },
  { id: "beijing", name: "Beijing", country: "CN", lat: 39.9042, lon: 116.4074, tz: "Asia/Shanghai", locale: "zh-CN" },
  { id: "seoul", name: "Seoul", country: "KR", lat: 37.5665, lon: 126.978, tz: "Asia/Seoul", locale: "ko-KR" },
  { id: "tokyo", name: "Tokyo", country: "JP", lat: 35.6762, lon: 139.6503, tz: "Asia/Tokyo", locale: "ja-JP" },
  { id: "osaka", name: "Osaka", country: "JP", lat: 34.6937, lon: 135.5023, tz: "Asia/Tokyo", locale: "ja-JP" },
  { id: "sydney", name: "Sydney", country: "AU", lat: -33.8688, lon: 151.2093, tz: "Australia/Sydney", locale: "en-AU" },
  { id: "melbourne", name: "Melbourne", country: "AU", lat: -37.8136, lon: 144.9631, tz: "Australia/Melbourne", locale: "en-AU" },
  { id: "auckland", name: "Auckland", country: "NZ", lat: -36.8485, lon: 174.7633, tz: "Pacific/Auckland", locale: "en-NZ" },
  { id: "reykjavik", name: "Reykjavík", country: "IS", lat: 64.1466, lon: -21.9426, tz: "Atlantic/Reykjavik", locale: "is-IS" },
];

Shield.cityById = function cityById(id) {
  return Shield.CITIES.find((city) => city.id === id) || null;
};

Shield.cityForCountry = function cityForCountry(country) {
  return Shield.CITIES.find((city) => city.country === String(country || "").toUpperCase()) || null;
};
})();
