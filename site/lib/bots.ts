// Who may read the site's pages, and which requests are someone probing for
// files that don't exist here. Search engines and link-preview bots are
// welcome; scraping libraries, site copiers and AI-training crawlers are not.
// A user agent can be faked, so this stops the common tools, not a determined
// person; the firewall's rate limits in Vercel cover the rest.

const BLOCKED_AGENTS = [
  // scraping libraries and headless browsers
  'python-requests',
  'python-urllib',
  'aiohttp',
  'httpx',
  'scrapy',
  'curl/',
  'wget/',
  'go-http-client',
  'okhttp',
  'java/',
  'libwww-perl',
  'node-fetch',
  'axios/',
  'undici',
  'headlesschrome',
  'phantomjs',
  'puppeteer',
  'playwright',
  'selenium',
  // site copiers
  'httrack',
  'webcopier',
  'sitesucker',
  'teleport',
  'offline explorer',
  'webzip',
  // bulk SEO and data crawlers
  'mj12bot',
  'ahrefsbot',
  'semrushbot',
  'dotbot',
  'petalbot',
  'dataforseobot',
  'blexbot',
  'megaindex',
  'serpstatbot',
  'screaming frog',
  'zoominfobot',
  // AI-training crawlers
  'gptbot',
  'ccbot',
  'claudebot',
  'claude-web',
  'anthropic-ai',
  'bytespider',
  'perplexitybot',
  'amazonbot',
  'applebot-extended',
  'cohere-ai',
  'diffbot',
  'imagesiftbot',
  'img2dataset',
  'omgili',
  'timpibot',
  'youbot',
  'meta-externalagent',
]

/** The crawlers robots.txt shuts out of the whole site. */
export const AI_CRAWLERS = [
  'GPTBot',
  'CCBot',
  'ClaudeBot',
  'Claude-Web',
  'anthropic-ai',
  'Google-Extended',
  'Applebot-Extended',
  'Bytespider',
  'PerplexityBot',
  'Amazonbot',
  'cohere-ai',
  'Diffbot',
  'ImagesiftBot',
  'Meta-ExternalAgent',
  'Omgilibot',
  'Timpibot',
  'YouBot',
]

export function isBlockedAgent(userAgent: string | null): boolean {
  // Real browsers and the crawlers we want always send one.
  if (!userAgent || userAgent.trim().length === 0) return true
  const agent = userAgent.toLowerCase()
  return BLOCKED_AGENTS.some((blocked) => agent.includes(blocked))
}

// Paths that only exist on other kinds of servers. Asking for them is how
// automated scanners and search-engine "dorks" look for leaked config,
// backups and admin panels.
const PROBE_PATTERNS = [
  /^\/\.(?!well-known\/)/, // dotfiles: .env, .git, .svn, .DS_Store, .htaccess
  /^\/(wp-|wordpress|xmlrpc\.php)/i,
  /^\/(phpmyadmin|pma|myadmin|adminer)/i,
  /^\/(admin|administrator|cpanel|server-status|server-info|cgi-bin)(\/|$)/i,
  /^\/(backup|backups|dump|db|database|config|conf|secrets?|private|logs?)(\/|\.|$)/i,
  /\.(php|asp|aspx|jsp|cgi|env|ini|conf|cfg|bak|old|orig|save|swp|sql|sqlite|db|log|git|svn|pem|key|p12|pfx)$/i,
  /(^|\/)(id_rsa|id_ed25519|credentials|\.npmrc|\.pypirc|docker-compose\.ya?ml|composer\.json|package-lock\.json)$/i,
]

export function isProbePath(pathname: string): boolean {
  return PROBE_PATTERNS.some((pattern) => pattern.test(pathname))
}
