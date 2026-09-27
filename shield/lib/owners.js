// Who is behind a third-party host, for the "who's watching" list: the
// company, what kind of thing it does, and where it is based. Matched by
// domain suffix; unknown hosts are shown as themselves.
"use strict";
(() => {
const Shield = (globalThis.Shield = globalThis.Shield || {});

const OWNERS = [
  ["google-analytics.com", "Google", "analytics", "US"], ["googletagmanager.com", "Google", "analytics", "US"], ["doubleclick.net", "Google", "advertising", "US"],
  ["googlesyndication.com", "Google", "advertising", "US"], ["googleadservices.com", "Google", "advertising", "US"], ["gstatic.com", "Google", "content", "US"],
  ["googleapis.com", "Google", "content", "US"], ["google.com", "Google", "service", "US"], ["youtube.com", "Google", "media", "US"], ["ggpht.com", "Google", "content", "US"],
  ["googleusercontent.com", "Google", "content", "US"], ["googleoptimize.com", "Google", "analytics", "US"], ["2mdn.net", "Google", "advertising", "US"],
  ["facebook.com", "Meta", "social", "US"], ["facebook.net", "Meta", "advertising", "US"], ["fbcdn.net", "Meta", "content", "US"], ["instagram.com", "Meta", "social", "US"],
  ["amazon-adsystem.com", "Amazon", "advertising", "US"], ["amazonaws.com", "Amazon Web Services", "cloud", "US"], ["cloudfront.net", "Amazon Web Services", "cdn", "US"],
  ["media-amazon.com", "Amazon", "content", "US"], ["ssl-images-amazon.com", "Amazon", "content", "US"],
  ["bing.com", "Microsoft", "advertising", "US"], ["clarity.ms", "Microsoft", "session replay", "US"], ["microsoft.com", "Microsoft", "service", "US"],
  ["msecnd.net", "Microsoft", "cdn", "US"], ["azureedge.net", "Microsoft", "cdn", "US"], ["linkedin.com", "Microsoft", "social", "US"], ["licdn.com", "Microsoft", "advertising", "US"],
  ["twitter.com", "X", "social", "US"], ["x.com", "X", "social", "US"], ["twimg.com", "X", "content", "US"], ["ads-twitter.com", "X", "advertising", "US"],
  ["tiktok.com", "ByteDance", "social", "CN"], ["tiktokcdn.com", "ByteDance", "content", "CN"], ["byteoversea.com", "ByteDance", "analytics", "CN"],
  ["snapchat.com", "Snap", "advertising", "US"], ["sc-static.net", "Snap", "advertising", "US"], ["pinterest.com", "Pinterest", "advertising", "US"], ["pinimg.com", "Pinterest", "content", "US"],
  ["adsrvr.org", "The Trade Desk", "advertising", "US"], ["criteo.com", "Criteo", "advertising", "FR"], ["criteo.net", "Criteo", "advertising", "FR"],
  ["taboola.com", "Taboola", "advertising", "IL"], ["outbrain.com", "Outbrain", "advertising", "IL"], ["pubmatic.com", "PubMatic", "advertising", "US"],
  ["rubiconproject.com", "Magnite", "advertising", "US"], ["openx.net", "OpenX", "advertising", "US"], ["casalemedia.com", "Index Exchange", "advertising", "CA"],
  ["indexww.com", "Index Exchange", "advertising", "CA"], ["appnexus.com", "Xandr (Microsoft)", "advertising", "US"], ["adnxs.com", "Xandr (Microsoft)", "advertising", "US"],
  ["yieldmo.com", "Yieldmo", "advertising", "US"], ["sharethrough.com", "Sharethrough", "advertising", "CA"], ["triplelift.com", "TripleLift", "advertising", "US"],
  ["3lift.com", "TripleLift", "advertising", "US"], ["smartadserver.com", "Equativ", "advertising", "FR"], ["teads.tv", "Teads", "advertising", "LU"],
  ["media.net", "Media.net", "advertising", "AE"], ["mgid.com", "MGID", "advertising", "US"], ["revcontent.com", "Revcontent", "advertising", "US"],
  ["quantserve.com", "Quantcast", "advertising", "US"], ["quantcount.com", "Quantcast", "advertising", "US"], ["scorecardresearch.com", "Comscore", "analytics", "US"],
  ["chartbeat.com", "Chartbeat", "analytics", "US"], ["chartbeat.net", "Chartbeat", "analytics", "US"], ["parsely.com", "Parse.ly", "analytics", "US"],
  ["newrelic.com", "New Relic", "monitoring", "US"], ["nr-data.net", "New Relic", "monitoring", "US"], ["datadoghq.com", "Datadog", "monitoring", "US"],
  ["sentry.io", "Sentry", "monitoring", "US"], ["bugsnag.com", "Bugsnag", "monitoring", "US"], ["rollbar.com", "Rollbar", "monitoring", "US"],
  ["hotjar.com", "Hotjar (Contentsquare)", "session replay", "MT"], ["fullstory.com", "FullStory", "session replay", "US"], ["logrocket.com", "LogRocket", "session replay", "US"],
  ["mouseflow.com", "Mouseflow", "session replay", "DK"], ["smartlook.com", "Smartlook", "session replay", "CZ"], ["contentsquare.net", "Contentsquare", "session replay", "FR"],
  ["quantummetric.com", "Quantum Metric", "session replay", "US"], ["crazyegg.com", "Crazy Egg", "session replay", "US"], ["inspectlet.com", "Inspectlet", "session replay", "US"],
  ["mixpanel.com", "Mixpanel", "analytics", "US"], ["segment.com", "Twilio Segment", "analytics", "US"], ["segment.io", "Twilio Segment", "analytics", "US"],
  ["amplitude.com", "Amplitude", "analytics", "US"], ["heap.io", "Heap", "analytics", "US"], ["heapanalytics.com", "Heap", "analytics", "US"],
  ["kissmetrics.com", "Kissmetrics", "analytics", "US"], ["matomo.cloud", "Matomo", "analytics", "NZ"], ["plausible.io", "Plausible", "analytics", "EE"],
  ["hubspot.com", "HubSpot", "marketing", "US"], ["hs-scripts.com", "HubSpot", "marketing", "US"], ["hsforms.com", "HubSpot", "marketing", "US"], ["hs-analytics.net", "HubSpot", "analytics", "US"],
  ["marketo.net", "Adobe Marketo", "marketing", "US"], ["mktoresp.com", "Adobe Marketo", "marketing", "US"], ["pardot.com", "Salesforce", "marketing", "US"],
  ["salesforce.com", "Salesforce", "marketing", "US"], ["exacttarget.com", "Salesforce", "marketing", "US"], ["demdex.net", "Adobe", "advertising", "US"],
  ["omtrdc.net", "Adobe Analytics", "analytics", "US"], ["adobedtm.com", "Adobe", "analytics", "US"], ["everesttech.net", "Adobe", "advertising", "US"], ["typekit.net", "Adobe", "fonts", "US"],
  ["optimizely.com", "Optimizely", "testing", "US"], ["vwo.com", "VWO", "testing", "IN"], ["visualwebsiteoptimizer.com", "VWO", "testing", "IN"], ["abtasty.com", "AB Tasty", "testing", "FR"],
  ["intercom.io", "Intercom", "chat", "US"], ["intercomcdn.com", "Intercom", "chat", "US"], ["drift.com", "Drift", "chat", "US"], ["zendesk.com", "Zendesk", "support", "US"],
  ["zdassets.com", "Zendesk", "support", "US"], ["livechatinc.com", "LiveChat", "chat", "PL"], ["tawk.to", "tawk.to", "chat", "US"], ["crisp.chat", "Crisp", "chat", "FR"],
  ["freshworks.com", "Freshworks", "support", "US"], ["freshdesk.com", "Freshworks", "support", "US"],
  ["braze.com", "Braze", "marketing", "US"], ["appboy.com", "Braze", "marketing", "US"], ["onesignal.com", "OneSignal", "notifications", "US"], ["pushwoosh.com", "Pushwoosh", "notifications", "US"],
  ["klaviyo.com", "Klaviyo", "marketing", "US"], ["mailchimp.com", "Mailchimp (Intuit)", "marketing", "US"], ["list-manage.com", "Mailchimp (Intuit)", "marketing", "US"],
  ["sendgrid.net", "Twilio SendGrid", "email", "US"], ["attn.tv", "Attentive", "marketing", "US"], ["yotpo.com", "Yotpo", "reviews", "IL"], ["bazaarvoice.com", "Bazaarvoice", "reviews", "US"],
  ["trustpilot.com", "Trustpilot", "reviews", "DK"], ["cloudflare.com", "Cloudflare", "cdn", "US"], ["cloudflareinsights.com", "Cloudflare", "analytics", "US"],
  ["akamaihd.net", "Akamai", "cdn", "US"], ["akamaized.net", "Akamai", "cdn", "US"], ["akstat.io", "Akamai", "monitoring", "US"], ["go-mpulse.net", "Akamai", "monitoring", "US"],
  ["fastly.net", "Fastly", "cdn", "US"], ["jsdelivr.net", "jsDelivr", "cdn", "PL"], ["unpkg.com", "Cloudflare (unpkg)", "cdn", "US"], ["cdnjs.cloudflare.com", "Cloudflare", "cdn", "US"],
  ["bootstrapcdn.com", "jsDelivr", "cdn", "PL"], ["jquery.com", "OpenJS Foundation", "cdn", "US"], ["fontawesome.com", "Fonticons", "fonts", "US"], ["fonts.googleapis.com", "Google Fonts", "fonts", "US"],
  ["stripe.com", "Stripe", "payments", "US"], ["paypal.com", "PayPal", "payments", "US"], ["paypalobjects.com", "PayPal", "payments", "US"], ["braintreegateway.com", "PayPal Braintree", "payments", "US"],
  ["klarna.com", "Klarna", "payments", "SE"], ["afterpay.com", "Afterpay (Block)", "payments", "AU"], ["affirm.com", "Affirm", "payments", "US"], ["adyen.com", "Adyen", "payments", "NL"],
  ["recaptcha.net", "Google reCAPTCHA", "captcha", "US"], ["hcaptcha.com", "hCaptcha", "captcha", "US"], ["challenges.cloudflare.com", "Cloudflare Turnstile", "captcha", "US"],
  ["shopify.com", "Shopify", "commerce", "CA"], ["shopifycdn.com", "Shopify", "commerce", "CA"], ["shopifysvc.com", "Shopify", "commerce", "CA"],
  ["wix.com", "Wix", "hosting", "IL"], ["wixstatic.com", "Wix", "hosting", "IL"], ["squarespace.com", "Squarespace", "hosting", "US"], ["squarespace-cdn.com", "Squarespace", "hosting", "US"],
  ["liadm.com", "LiveIntent", "advertising", "US"], ["id5-sync.com", "ID5", "identity", "GB"], ["rlcdn.com", "LiveRamp", "identity", "US"], ["liveramp.com", "LiveRamp", "identity", "US"],
  ["bluekai.com", "Oracle", "advertising", "US"], ["addthis.com", "Oracle", "advertising", "US"], ["eloqua.com", "Oracle", "marketing", "US"], ["moatads.com", "Oracle Moat", "advertising", "US"],
  ["doubleverify.com", "DoubleVerify", "advertising", "US"], ["adsafeprotected.com", "Integral Ad Science", "advertising", "US"], ["iasds01.com", "Integral Ad Science", "advertising", "US"],
  ["permutive.com", "Permutive", "advertising", "GB"], ["permutive.app", "Permutive", "advertising", "GB"], ["lotame.com", "Lotame", "advertising", "US"], ["crwdcntrl.net", "Lotame", "advertising", "US"],
  ["tapad.com", "Tapad (Experian)", "identity", "US"], ["exelator.com", "Nielsen", "advertising", "US"], ["imrworldwide.com", "Nielsen", "analytics", "US"],
  ["yandex.ru", "Yandex", "analytics", "RU"], ["yandex.net", "Yandex", "analytics", "RU"], ["mc.yandex.ru", "Yandex Metrica", "session replay", "RU"],
  ["baidu.com", "Baidu", "analytics", "CN"], ["hm.baidu.com", "Baidu", "analytics", "CN"], ["cnzz.com", "Alibaba Umeng", "analytics", "CN"], ["umeng.com", "Alibaba Umeng", "analytics", "CN"],
  ["alicdn.com", "Alibaba", "content", "CN"], ["mmstat.com", "Alibaba", "analytics", "CN"], ["qq.com", "Tencent", "analytics", "CN"], ["gtimg.com", "Tencent", "content", "CN"],
  ["naver.com", "Naver", "analytics", "KR"], ["naver.net", "Naver", "content", "KR"], ["kakao.com", "Kakao", "analytics", "KR"], ["line.me", "LY Corporation", "analytics", "JP"],
  ["vk.com", "VK", "social", "RU"], ["mail.ru", "VK", "analytics", "RU"], ["ok.ru", "VK", "social", "RU"],
  ["branch.io", "Branch", "attribution", "US"], ["adjust.com", "Adjust (AppLovin)", "attribution", "DE"], ["appsflyer.com", "AppsFlyer", "attribution", "IL"], ["kochava.com", "Kochava", "attribution", "US"],
  ["bugherd.com", "BugHerd", "feedback", "AU"], ["usabilla.com", "Usabilla", "feedback", "NL"], ["qualtrics.com", "Qualtrics", "surveys", "US"], ["surveymonkey.com", "SurveyMonkey", "surveys", "US"],
  ["vimeo.com", "Vimeo", "media", "US"], ["vimeocdn.com", "Vimeo", "media", "US"], ["wistia.com", "Wistia", "media", "US"], ["brightcove.com", "Brightcove", "media", "US"], ["jwplayer.com", "JW Player", "media", "US"],
  ["spotify.com", "Spotify", "media", "SE"], ["soundcloud.com", "SoundCloud", "media", "DE"], ["disqus.com", "Disqus", "comments", "US"], ["disquscdn.com", "Disqus", "comments", "US"],
  ["gravatar.com", "Automattic", "content", "US"], ["wp.com", "Automattic", "hosting", "US"], ["cookielaw.org", "OneTrust", "consent", "US"], ["onetrust.com", "OneTrust", "consent", "US"],
  ["cookiebot.com", "Usercentrics Cookiebot", "consent", "DK"], ["usercentrics.eu", "Usercentrics", "consent", "DE"], ["trustarc.com", "TrustArc", "consent", "US"], ["quantcast.mgr.consensu.org", "Quantcast", "consent", "US"],
  ["sourcepoint.mgr.consensu.org", "Sourcepoint", "consent", "US"], ["privacy-mgmt.com", "Sourcepoint", "consent", "US"], ["didomi.io", "Didomi", "consent", "FR"], ["consentmanager.net", "consentmanager", "consent", "DE"],
];

Shield.ownerOf = function ownerOf(host) {
  const name = String(host || "").toLowerCase();
  let best = null;
  for (const [suffix, owner, kind, country] of OWNERS) {
    if ((name === suffix || name.endsWith("." + suffix)) && (!best || suffix.length > best.suffix.length)) best = { suffix, owner, kind, country };
  }
  return best;
};
})();
