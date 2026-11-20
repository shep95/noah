// The site speaks sixteen languages. A language lives in the first path
// segment (/es, /ja/download); English has no prefix. middleware.ts strips the
// prefix and passes the language on in a header, so every page is one file.
// The home and download pages are translated whole; the long pages keep their
// English body under a translated frame and say so in one line.
export const LANGUAGES = {
  en: 'english',
  es: 'español',
  pt: 'português',
  fr: 'français',
  de: 'deutsch',
  it: 'italiano',
  nl: 'nederlands',
  pl: 'polski',
  tr: 'türkçe',
  ru: 'русский',
  uk: 'українська',
  ar: 'العربية',
  hi: 'हिन्दी',
  ja: '日本語',
  ko: '한국어',
  zh: '中文',
} as const

export type Language = keyof typeof LANGUAGES
export const LANGUAGE_CODES = Object.keys(LANGUAGES) as Language[]
export const DEFAULT_LANGUAGE: Language = 'en'
export const RIGHT_TO_LEFT: Language[] = ['ar']
/** Where the visitor's own choice of language is remembered. */
export const LANGUAGE_STORAGE_KEY = 'noah-language'
export const LANGUAGE_HEADER = 'x-language'

/** Open Graph locales, where they differ from the plain code. */
const OPEN_GRAPH_LOCALES: Record<Language, string> = {
  en: 'en_US',
  es: 'es_ES',
  pt: 'pt_BR',
  fr: 'fr_FR',
  de: 'de_DE',
  it: 'it_IT',
  nl: 'nl_NL',
  pl: 'pl_PL',
  tr: 'tr_TR',
  ru: 'ru_RU',
  uk: 'uk_UA',
  ar: 'ar_AR',
  hi: 'hi_IN',
  ja: 'ja_JP',
  ko: 'ko_KR',
  zh: 'zh_CN',
}

export function isLanguage(value: string | null | undefined): value is Language {
  return !!value && Object.prototype.hasOwnProperty.call(LANGUAGES, value)
}

/** Splits a request path into its language and the path the page knows. */
export function splitLanguage(pathname: string): { language: Language; path: string } {
  const [, first = '', ...rest] = pathname.split('/')
  if (isLanguage(first)) {
    const remainder = rest.join('/')
    return { language: first, path: remainder ? `/${remainder}` : '/' }
  }
  return { language: DEFAULT_LANGUAGE, path: pathname || '/' }
}

/** A site path in a language: `/download` in Japanese is `/ja/download`. */
export function localized(path: string, language: Language): string {
  if (language === DEFAULT_LANGUAGE) return path
  return path === '/' ? `/${language}` : `/${language}${path}`
}

export function openGraphLocale(language: Language): string {
  return OPEN_GRAPH_LOCALES[language]
}

/** hreflang alternates for a page that exists in every language. */
export function languageAlternates(path: string): Record<string, string> {
  const alternates: Record<string, string> = { 'x-default': path }
  for (const language of LANGUAGE_CODES) alternates[language] = localized(path, language)
  return alternates
}

export type Dictionary = {
  nav: { features: string; shepherd: string; faq: string; founder: string; download: string; language: string }
  footer: { by: string; security: string; terms: string; source: string; stats: string }
  hero: { claim: string; proof: string }
  features: { title: string; body: string }[]
  panels: { connected: string; yourKey: string; local: string; nothingMetered: string; webSearch: string }
  privacy: { title: string; body: string; accounts: string; telemetry: string; localModels: string; paywalls: string }
  cta: { eyebrow: string; lineOne: string; lineTwo: string }
  /** Untranslated languages fall back to the english list. */
  inside?: { eyebrow: string; title: string; lede: string; summary: string; items: string[]; note: string }
  /** Untranslated languages fall back to the english list. */
  against?: { eyebrow: string; title: string; lede: string; summary: string; items: string[]; note: string }
  /** Untranslated languages fall back to the english text. */
  warning?: { eyebrow: string; title: string; lede: string; summary: string; layers: { who: string; what: string }[]; root: string; note: string }
  download: { forPlatform: string; all: string; none: string; button: string }
  downloadPage: {
    title: string
    metaTitle: string
    description: string
    meta: string
    heading: string
    lede: string
    check: string
    needs: string
  }
  english: string
  home: { description: string }
}

const en: Dictionary = {
  nav: { features: 'features', shepherd: 'shepherd', faq: 'faq', founder: 'founder', download: 'download', language: 'language' },
  footer: { by: 'by house of asher', security: 'security', terms: 'terms', source: 'source', stats: 'stats' },
  hero: {
    claim: 'a shepherd for your code.',
    proof: 'it reads your whole project and writes like it already lived there.',
  },
  features: [
    {
      title: 'it reads the whole\nproject first',
      body: 'import a repo or open a folder and shepherd studies it before it writes a line — the architecture, the naming, the patterns you already chose. then it matches them, so its code reads like the hand that started the project.',
    },
    {
      title: 'your models.\nyour keys.',
      body: 'bring a key from any of nearly fifty providers, western or chinese, or point shepherd at a model running on your own machine. local models are limited only by your hardware — 7b or 405b, whatever your memory can hold.',
    },
    {
      title: 'the editor takes\non your image',
      body: 'set any wallpaper and the interface reads its palette. the code stays legible, calm — always sitting cleanly on top. the environment becomes yours.',
    },
    {
      title: 'it looks outward\nwhen it has to',
      body: 'when the answer is not in your project, shepherd searches the web. no account, no key. every search asks you first, and the sources sit right under the answer.',
    },
  ],
  panels: {
    connected: 'connected models',
    yourKey: 'your key',
    local: 'local',
    nothingMetered: 'nothing metered. nothing phoned home.',
    webSearch: 'web search · asks first',
  },
  privacy: {
    title: 'free. private. yours.',
    body: 'no account, no paywall, no telemetry. we do not own your projects or keep records of them. the models you connect answer to you, under your key — not to us.',
    accounts: 'accounts',
    telemetry: 'telemetry',
    localModels: 'local models',
    paywalls: 'paywalls',
  },
  cta: { eyebrow: 'when you are ready', lineOne: 'come to the quiet', lineTwo: 'and start.' },
  inside: {
    eyebrow: 'what is inside',
    title: 'a few things it can do.',
    lede: 'it is still learning, and so are we. none of this is finished, and some of it will not suit you. here is the plain list, for whoever wants it.',
    summary: 'show the list',
    items: [
      'reads a project before writing to it, and keeps notes about it in .noah/',
      'works with nearly fifty model providers on your own key, or models on your own machine',
      'an offline mode where nothing leaves the machine',
      'searches the web when the answer is not in your project, with sources underneath',
      'runs your project’s own checks after it edits, and tries again when they fail',
      'runs terminal commands in a sandbox, and asks before reaching further',
      'a full automation switch, for when you would rather it did not stop to ask',
      'a teaching mode that leaves the important part for you to write',
      'an r&d mode that forms a guess, tests it small, and writes down what it found',
      'hands in evidence with each change: the commands that really ran, tied to each claim, and what it did not verify',
      'a second model from another family reviews the work before it is called done',
      'a provenance log of every edit and command, and a record of how past work held up',
      'mission control: every conversation, the decisions waiting on you, spend against your budget',
      'spending limits per month and per thread for paid models',
      'small tested add-ons it writes once and reuses, so a calculation comes out the same each time',
      'a brain file: standing instructions you write once and it keeps, and a brain it can read in full',
      'a browser room to see interface code as it is built, which it asks before reading when you opened the page',
      'a device room: security checks, startup items, open ports, duplicate files, an ad blocker for every app',
      'voice in and out, through your own provider’s key',
      'a whiteboard, a notepad, a chat outside any project, and a place to write documents and slides',
      'starts a new project from a single prompt',
      'a settings search that understands a misspelled word',
      'takes its colors from any wallpaper you give it',
      'a receipt under each reply that did work: what ran, what passed, which files changed, and what nobody checked',
      'a second opinion on any reply, from a model of another family, one click away',
      'replays the commands behind an old piece of evidence and tells you what still holds',
      'turns the invariants in your spec into property tests, and plain-words notes on each part of a commit',
      'tells you what a big task will likely cost before it starts, and waits for your yes',
      'a timeline of the project: every edit, every piece of evidence, every commit, and how each held up',
      'looks a name up through several search engines and countries at once, without cookies',
      'watches a page you choose and tells you when it changes',
      'a drawer in the device room: programs, startup items, ports and browser extensions that arrived since you last looked',
      'checks that your backup exists, how old it is, and on a mac whether a file actually comes back from it',
      'on windows, a leash that cuts one program off the network until you let it go',
      'a money room for ai spend, receipts and subscriptions, a reading room for books and papers, and a user manual it keeps for your project',
      'download a single file from a project, not only the whole thing as a zip',
      'the shield, a browser extension: a fail-closed tunnel, blocking, capture guards, and a map of your own footprint across the web',
      'the shield can tell noah what it saw on a site, fills forms from details kept in your browser, and swaps a stored secret pasted into an ai chat for its name',
    ],
    note: 'there may be more in there than this. it is easier to find out by using it.',
  },
  against: {
    eyebrow: 'why you should not use us',
    title: 'reasons to go elsewhere.',
    lede: 'we would rather you knew before you downloaded than found out after. here are the honest ones, and where you might be better off.',
    summary: 'show the reasons',
    items: [
      'there is no mac version. if you are on a mac, zed or cursor will serve you well today.',
      'you bring your own api key. if you want to pay one bill and never think about models, cursor and windsurf bundle that for you.',
      'noah is a fork of zed. if you want the original, kept by a larger team and updated sooner, it is free at zed.dev.',
      'it is young, and a small team keeps it. things break. if you need something that does not, vs code with copilot or a jetbrains editor is the safer bet.',
      'there is no account, so there is no sync between your computers. if you want your settings to follow you, most others do that and we do not.',
      'if you live in the terminal, claude code or codex may suit you better than an editor with an agent inside it.',
      'the free model keys we point you to are rate limited, and some of those companies learn from what you send. a paid key, or a local model, is the private path.',
      'you will do more setup than with the paid tools. a key, a model choice, maybe ollama. some people like that. some do not.',
    ],
    note: 'if none of that puts you off, we are glad. if some of it does, the tools above are good, and we mean that.',
  },
  warning: {
    eyebrow: 'before you download',
    title: 'it will be called a security risk.',
    lede: 'your browser, and then windows, will warn you about the file. it is not harmful. nobody has paid to vouch for it, and that is the whole of it. here is who is saying it, from the surface down to the root.',
    summary: 'show who says so, and why',
    layers: [
      {
        who: 'your browser',
        what: 'chrome, edge or firefox says the file "isn\'t commonly downloaded" or "may be dangerous". it has not looked inside. it asked a reputation service (google safe browsing, or microsoft smartscreen) whether it has seen this exact file downloaded many times before. a new release of a small program has no history, so the answer is no.',
      },
      {
        who: 'windows smartscreen',
        what: '"windows protected your pc. unknown publisher." windows looks for a digital signature on the file. ours has none, so it cannot name a publisher, and it treats a program it cannot name as suspect. more info, then run anyway.',
      },
      {
        who: 'the certificate sellers',
        what: 'a signature needs a code-signing certificate, sold only by a few authorities: digicert, sectigo, globalsign, ssl.com, certum. a few hundred dollars a year, and smartscreen still warns until the certificate earns reputation. the "extended" kind skips the warning at once, costs more, ships on a hardware key, and needs a registered company first.',
      },
      {
        who: 'the root',
        what: 'microsoft\'s trusted root program decides which of those authorities windows believes. apple does the same on the mac, and charges its own fee to notarise each build. those two, and the sellers they admit, are the end of the chain.',
      },
    ],
    root: 'nowhere in that chain does anyone read what the program does. signed malware exists, and honest unsigned software is warned about every day. the warning means nobody paid. that is all it means.',
    note: 'if you would rather check than click through: compare the sha-256 on the download page with the file you got, or read the source and build it yourself. that is the only real proof, and it is why noah is open. we will sign releases once the certificate is paid for.',
  },
  download: {
    forPlatform: 'download for {platform}',
    all: 'all downloads →',
    none: 'there is no {platform} build of noah yet.',
    button: 'download',
  },
  downloadPage: {
    title: 'Download noah',
    metaTitle: 'download noah for windows and linux',
    description: 'download noah {version}, the free ai code editor with shepherd. an installer for windows 10 and 11, a .deb and a .tar.xz for linux.',
    meta: 'free · no account · no telemetry',
    heading: 'Download noah',
    lede: 'The editor and shepherd, its agent, in one install. After it opens, add an API key from any provider you use, or point it at a local model, and start.',
    check: 'Check your download',
    needs: 'What it needs',
  },
  english: 'this page is in english for now.',
  home: {
    description:
      'noah is a free ai code editor for windows and linux. shepherd, its agent, reads your whole project first. nearly fifty providers on your own key, or local models.',
  },
}

const es: Dictionary = {
  nav: { features: 'funciones', shepherd: 'shepherd', faq: 'preguntas', founder: 'fundador', download: 'descargar', language: 'idioma' },
  footer: { by: 'por house of asher', security: 'seguridad', terms: 'términos', source: 'código', stats: 'estadísticas' },
  hero: {
    claim: 'un pastor para tu código.',
    proof: 'lee todo tu proyecto y escribe como si siempre hubiera vivido ahí.',
  },
  features: [
    {
      title: 'primero lee\ntodo el proyecto',
      body: 'importa un repositorio o abre una carpeta y shepherd lo estudia antes de escribir una línea: la arquitectura, los nombres, los patrones que ya elegiste. luego los sigue, para que su código se lea como la mano que empezó el proyecto.',
    },
    {
      title: 'tus modelos.\ntus claves.',
      body: 'trae una clave de casi cincuenta proveedores, occidentales o chinos, o apunta shepherd a un modelo que corre en tu propia máquina. los modelos locales solo los limita tu hardware: 7b o 405b, lo que quepa en tu memoria.',
    },
    {
      title: 'el editor toma\ntu imagen',
      body: 'pon cualquier fondo y la interfaz lee su paleta. el código sigue legible, en calma, siempre limpio encima. el entorno se vuelve tuyo.',
    },
    {
      title: 'mira hacia fuera\ncuando hace falta',
      body: 'cuando la respuesta no está en tu proyecto, shepherd busca en la web. sin cuenta, sin clave. cada búsqueda te pregunta antes, y las fuentes quedan justo bajo la respuesta.',
    },
  ],
  panels: {
    connected: 'modelos conectados',
    yourKey: 'tu clave',
    local: 'local',
    nothingMetered: 'nada medido. nada enviado a casa.',
    webSearch: 'búsqueda web · pregunta antes',
  },
  privacy: {
    title: 'gratis. privado. tuyo.',
    body: 'sin cuenta, sin muro de pago, sin telemetría. no somos dueños de tus proyectos ni guardamos registros. los modelos que conectas te responden a ti, con tu clave, no a nosotros.',
    accounts: 'cuentas',
    telemetry: 'telemetría',
    localModels: 'modelos locales',
    paywalls: 'muros de pago',
  },
  cta: { eyebrow: 'cuando estés listo', lineOne: 'ven a la calma', lineTwo: 'y empieza.' },
  download: {
    forPlatform: 'descargar para {platform}',
    all: 'todas las descargas →',
    none: 'todavía no hay una versión de noah para {platform}.',
    button: 'descargar',
  },
  downloadPage: {
    title: 'Descargar noah',
    metaTitle: 'descargar noah para windows y linux',
    description: 'descarga noah {version}, el editor de código con ia gratuito con shepherd. un instalador para windows 10 y 11, un .deb y un .tar.xz para linux.',
    meta: 'gratis · sin cuenta · sin telemetría',
    heading: 'Descargar noah',
    lede: 'El editor y shepherd, su agente, en una sola instalación. Cuando se abra, añade una clave de API de cualquier proveedor que uses, o apúntalo a un modelo local, y empieza.',
    check: 'Comprueba tu descarga',
    needs: 'Qué necesita',
  },
  english: 'esta página está en inglés por ahora.',
  home: {
    description:
      'noah es un editor de código con ia gratuito para windows y linux. shepherd, su agente, lee primero todo tu proyecto. casi cincuenta proveedores con tu propia clave, o modelos locales.',
  },
}

const pt: Dictionary = {
  nav: { features: 'recursos', shepherd: 'shepherd', faq: 'perguntas', founder: 'fundador', download: 'baixar', language: 'idioma' },
  footer: { by: 'por house of asher', security: 'segurança', terms: 'termos', source: 'código', stats: 'estatísticas' },
  hero: {
    claim: 'um pastor para o seu código.',
    proof: 'ele lê o projeto inteiro e escreve como se sempre tivesse morado ali.',
  },
  features: [
    {
      title: 'primeiro ele lê\no projeto inteiro',
      body: 'importe um repositório ou abra uma pasta e o shepherd o estuda antes de escrever uma linha: a arquitetura, os nomes, os padrões que você já escolheu. depois ele os segue, para que seu código pareça vir da mão que começou o projeto.',
    },
    {
      title: 'seus modelos.\nsuas chaves.',
      body: 'traga uma chave de quase cinquenta provedores, ocidentais ou chineses, ou aponte o shepherd para um modelo rodando na sua própria máquina. modelos locais só são limitados pelo seu hardware: 7b ou 405b, o que couber na memória.',
    },
    {
      title: 'o editor assume\na sua imagem',
      body: 'defina qualquer papel de parede e a interface lê a paleta dele. o código continua legível, calmo, sempre limpo por cima. o ambiente passa a ser seu.',
    },
    {
      title: 'ele olha para fora\nquando precisa',
      body: 'quando a resposta não está no seu projeto, o shepherd pesquisa na web. sem conta, sem chave. cada pesquisa pede sua permissão antes, e as fontes ficam logo abaixo da resposta.',
    },
  ],
  panels: {
    connected: 'modelos conectados',
    yourKey: 'sua chave',
    local: 'local',
    nothingMetered: 'nada medido. nada enviado para casa.',
    webSearch: 'pesquisa na web · pergunta antes',
  },
  privacy: {
    title: 'grátis. privado. seu.',
    body: 'sem conta, sem paywall, sem telemetria. não somos donos dos seus projetos nem guardamos registros deles. os modelos que você conecta respondem a você, com a sua chave, não a nós.',
    accounts: 'contas',
    telemetry: 'telemetria',
    localModels: 'modelos locais',
    paywalls: 'paywalls',
  },
  cta: { eyebrow: 'quando estiver pronto', lineOne: 'venha para o silêncio', lineTwo: 'e comece.' },
  download: {
    forPlatform: 'baixar para {platform}',
    all: 'todos os downloads →',
    none: 'ainda não há uma versão do noah para {platform}.',
    button: 'baixar',
  },
  downloadPage: {
    title: 'Baixar o noah',
    metaTitle: 'baixar o noah para windows e linux',
    description: 'baixe o noah {version}, o editor de código com ia gratuito com o shepherd. um instalador para windows 10 e 11, um .deb e um .tar.xz para linux.',
    meta: 'grátis · sem conta · sem telemetria',
    heading: 'Baixar o noah',
    lede: 'O editor e o shepherd, seu agente, em uma só instalação. Quando abrir, adicione uma chave de API de qualquer provedor que você use, ou aponte para um modelo local, e comece.',
    check: 'Confira o seu download',
    needs: 'O que ele precisa',
  },
  english: 'esta página está em inglês por enquanto.',
  home: {
    description:
      'noah é um editor de código com ia gratuito para windows e linux. shepherd, seu agente, lê primeiro o projeto inteiro. quase cinquenta provedores com a sua própria chave, ou modelos locais.',
  },
}

const fr: Dictionary = {
  nav: { features: 'fonctions', shepherd: 'shepherd', faq: 'questions', founder: 'fondateur', download: 'télécharger', language: 'langue' },
  footer: { by: 'par house of asher', security: 'sécurité', terms: 'conditions', source: 'source', stats: 'statistiques' },
  hero: {
    claim: 'un berger pour votre code.',
    proof: 'il lit tout votre projet et écrit comme s’il y avait toujours vécu.',
  },
  features: [
    {
      title: 'il lit d’abord\ntout le projet',
      body: 'importez un dépôt ou ouvrez un dossier : shepherd l’étudie avant d’écrire une ligne, l’architecture, les noms, les habitudes que vous avez déjà choisies. puis il s’y conforme, pour que son code ressemble à la main qui a commencé le projet.',
    },
    {
      title: 'vos modèles.\nvos clés.',
      body: 'apportez une clé de l’un des cinquante fournisseurs, occidentaux ou chinois, ou pointez shepherd vers un modèle qui tourne sur votre propre machine. les modèles locaux ne sont limités que par votre matériel : 7b ou 405b, tout ce que votre mémoire peut contenir.',
    },
    {
      title: 'l’éditeur prend\nvotre image',
      body: 'choisissez n’importe quel fond d’écran et l’interface en lit la palette. le code reste lisible, calme, toujours net au premier plan. l’environnement devient le vôtre.',
    },
    {
      title: 'il regarde dehors\nquand il le faut',
      body: 'quand la réponse n’est pas dans votre projet, shepherd cherche sur le web. sans compte, sans clé. chaque recherche vous demande d’abord, et les sources se trouvent juste sous la réponse.',
    },
  ],
  panels: {
    connected: 'modèles connectés',
    yourKey: 'votre clé',
    local: 'local',
    nothingMetered: 'rien de mesuré. rien d’envoyé.',
    webSearch: 'recherche web · demande d’abord',
  },
  privacy: {
    title: 'gratuit. privé. à vous.',
    body: 'pas de compte, pas de paywall, pas de télémétrie. vos projets ne nous appartiennent pas et nous n’en gardons aucune trace. les modèles que vous connectez vous répondent à vous, sous votre clé, pas à nous.',
    accounts: 'comptes',
    telemetry: 'télémétrie',
    localModels: 'modèles locaux',
    paywalls: 'paywalls',
  },
  cta: { eyebrow: 'quand vous serez prêt', lineOne: 'venez dans le calme', lineTwo: 'et commencez.' },
  download: {
    forPlatform: 'télécharger pour {platform}',
    all: 'tous les téléchargements →',
    none: 'il n’y a pas encore de version de noah pour {platform}.',
    button: 'télécharger',
  },
  downloadPage: {
    title: 'Télécharger noah',
    metaTitle: 'télécharger noah pour windows et linux',
    description: 'téléchargez noah {version}, l’éditeur de code ia gratuit avec shepherd. un installeur pour windows 10 et 11, un .deb et un .tar.xz pour linux.',
    meta: 'gratuit · sans compte · sans télémétrie',
    heading: 'Télécharger noah',
    lede: 'L’éditeur et shepherd, son agent, en une seule installation. Une fois ouvert, ajoutez une clé d’API de n’importe quel fournisseur que vous utilisez, ou pointez-le vers un modèle local, et commencez.',
    check: 'Vérifiez votre téléchargement',
    needs: 'Ce qu’il lui faut',
  },
  english: 'cette page est en anglais pour le moment.',
  home: {
    description:
      'noah est un éditeur de code ia gratuit pour windows et linux. shepherd, son agent, lit d’abord tout votre projet. près de cinquante fournisseurs avec votre propre clé, ou des modèles locaux.',
  },
}

const de: Dictionary = {
  nav: { features: 'funktionen', shepherd: 'shepherd', faq: 'fragen', founder: 'gründer', download: 'herunterladen', language: 'sprache' },
  footer: { by: 'von house of asher', security: 'sicherheit', terms: 'bedingungen', source: 'quellcode', stats: 'statistik' },
  hero: {
    claim: 'ein hirte für deinen code.',
    proof: 'er liest dein ganzes projekt und schreibt, als hätte er schon immer darin gelebt.',
  },
  features: [
    {
      title: 'er liest zuerst\ndas ganze projekt',
      body: 'importiere ein repository oder öffne einen ordner, und shepherd studiert ihn, bevor er eine zeile schreibt: die architektur, die namen, die muster, die du schon gewählt hast. dann folgt er ihnen, damit sein code aussieht wie von der hand, die das projekt begonnen hat.',
    },
    {
      title: 'deine modelle.\ndeine schlüssel.',
      body: 'bring einen schlüssel von fast fünfzig anbietern mit, westlichen oder chinesischen, oder richte shepherd auf ein modell, das auf deinem eigenen rechner läuft. lokale modelle begrenzt nur deine hardware: 7b oder 405b, was immer in deinen speicher passt.',
    },
    {
      title: 'der editor nimmt\ndein bild an',
      body: 'wähle ein beliebiges hintergrundbild, und die oberfläche liest seine palette. der code bleibt lesbar, ruhig, immer sauber obenauf. die umgebung wird deine.',
    },
    {
      title: 'er schaut hinaus,\nwenn es sein muss',
      body: 'steht die antwort nicht in deinem projekt, sucht shepherd im web. ohne konto, ohne schlüssel. jede suche fragt dich vorher, und die quellen stehen direkt unter der antwort.',
    },
  ],
  panels: {
    connected: 'verbundene modelle',
    yourKey: 'dein schlüssel',
    local: 'lokal',
    nothingMetered: 'nichts gezählt. nichts nach hause gefunkt.',
    webSearch: 'websuche · fragt zuerst',
  },
  privacy: {
    title: 'kostenlos. privat. deins.',
    body: 'kein konto, keine paywall, keine telemetrie. deine projekte gehören nicht uns, und wir führen keine aufzeichnungen darüber. die modelle, die du verbindest, antworten dir, unter deinem schlüssel, nicht uns.',
    accounts: 'konten',
    telemetry: 'telemetrie',
    localModels: 'lokale modelle',
    paywalls: 'paywalls',
  },
  cta: { eyebrow: 'wenn du bereit bist', lineOne: 'komm in die stille', lineTwo: 'und fang an.' },
  download: {
    forPlatform: 'herunterladen für {platform}',
    all: 'alle downloads →',
    none: 'für {platform} gibt es noch keine version von noah.',
    button: 'herunterladen',
  },
  downloadPage: {
    title: 'noah herunterladen',
    metaTitle: 'noah für windows und linux herunterladen',
    description: 'lade noah {version} herunter, den kostenlosen ki-code-editor mit shepherd. ein installer für windows 10 und 11, ein .deb und ein .tar.xz für linux.',
    meta: 'kostenlos · kein konto · keine telemetrie',
    heading: 'noah herunterladen',
    lede: 'Der Editor und shepherd, sein Agent, in einer Installation. Sobald er offen ist, füge einen API-Schlüssel eines Anbieters hinzu, den du nutzt, oder richte ihn auf ein lokales Modell, und leg los.',
    check: 'Prüfe deinen Download',
    needs: 'Was er braucht',
  },
  english: 'diese seite ist vorerst auf englisch.',
  home: {
    description:
      'noah ist ein kostenloser ki-code-editor für windows und linux. shepherd, sein agent, liest zuerst dein ganzes projekt. fast fünfzig anbieter mit deinem eigenen schlüssel, oder lokale modelle.',
  },
}

const it: Dictionary = {
  nav: { features: 'funzioni', shepherd: 'shepherd', faq: 'domande', founder: 'fondatore', download: 'scarica', language: 'lingua' },
  footer: { by: 'di house of asher', security: 'sicurezza', terms: 'termini', source: 'sorgente', stats: 'statistiche' },
  hero: {
    claim: 'un pastore per il tuo codice.',
    proof: 'legge tutto il tuo progetto e scrive come se ci avesse sempre vissuto.',
  },
  features: [
    {
      title: 'prima legge\ntutto il progetto',
      body: 'importa un repository o apri una cartella e shepherd la studia prima di scrivere una riga: l’architettura, i nomi, gli schemi che hai già scelto. poi li segue, così il suo codice sembra scritto dalla mano che ha iniziato il progetto.',
    },
    {
      title: 'i tuoi modelli.\nle tue chiavi.',
      body: 'porta una chiave di uno dei quasi cinquanta fornitori, occidentali o cinesi, oppure punta shepherd a un modello che gira sulla tua macchina. i modelli locali sono limitati solo dal tuo hardware: 7b o 405b, quanto entra nella memoria.',
    },
    {
      title: 'l’editor prende\nla tua immagine',
      body: 'imposta qualsiasi sfondo e l’interfaccia ne legge la palette. il codice resta leggibile, calmo, sempre pulito in primo piano. l’ambiente diventa tuo.',
    },
    {
      title: 'guarda fuori\nquando serve',
      body: 'quando la risposta non è nel tuo progetto, shepherd cerca sul web. senza account, senza chiave. ogni ricerca ti chiede prima, e le fonti stanno proprio sotto la risposta.',
    },
  ],
  panels: {
    connected: 'modelli collegati',
    yourKey: 'la tua chiave',
    local: 'locale',
    nothingMetered: 'niente misurato. niente inviato a casa.',
    webSearch: 'ricerca web · chiede prima',
  },
  privacy: {
    title: 'gratis. privato. tuo.',
    body: 'nessun account, nessun paywall, nessuna telemetria. i tuoi progetti non sono nostri e non ne teniamo traccia. i modelli che colleghi rispondono a te, con la tua chiave, non a noi.',
    accounts: 'account',
    telemetry: 'telemetria',
    localModels: 'modelli locali',
    paywalls: 'paywall',
  },
  cta: { eyebrow: 'quando sei pronto', lineOne: 'vieni nella quiete', lineTwo: 'e comincia.' },
  download: {
    forPlatform: 'scarica per {platform}',
    all: 'tutti i download →',
    none: 'non c’è ancora una versione di noah per {platform}.',
    button: 'scarica',
  },
  downloadPage: {
    title: 'Scarica noah',
    metaTitle: 'scarica noah per windows e linux',
    description: 'scarica noah {version}, l’editor di codice con ia gratuito con shepherd. un installer per windows 10 e 11, un .deb e un .tar.xz per linux.',
    meta: 'gratis · senza account · senza telemetria',
    heading: 'Scarica noah',
    lede: 'L’editor e shepherd, il suo agente, in una sola installazione. Quando si apre, aggiungi una chiave API di un fornitore che usi, o puntalo a un modello locale, e comincia.',
    check: 'Controlla il download',
    needs: 'Cosa serve',
  },
  english: 'questa pagina per ora è in inglese.',
  home: {
    description:
      'noah è un editor di codice con ia gratuito per windows e linux. shepherd, il suo agente, legge prima tutto il tuo progetto. quasi cinquanta fornitori con la tua chiave, o modelli locali.',
  },
}

const nl: Dictionary = {
  nav: { features: 'functies', shepherd: 'shepherd', faq: 'vragen', founder: 'oprichter', download: 'downloaden', language: 'taal' },
  footer: { by: 'door house of asher', security: 'beveiliging', terms: 'voorwaarden', source: 'broncode', stats: 'statistieken' },
  hero: {
    claim: 'een herder voor je code.',
    proof: 'hij leest je hele project en schrijft alsof hij er altijd al woonde.',
  },
  features: [
    {
      title: 'hij leest eerst\nhet hele project',
      body: 'importeer een repository of open een map en shepherd bestudeert hem voor hij één regel schrijft: de architectuur, de namen, de patronen die je al koos. daarna volgt hij ze, zodat zijn code leest als de hand die het project begon.',
    },
    {
      title: 'jouw modellen.\njouw sleutels.',
      body: 'breng een sleutel mee van een van bijna vijftig aanbieders, westers of chinees, of wijs shepherd naar een model op je eigen machine. lokale modellen worden alleen begrensd door je hardware: 7b of 405b, wat je geheugen aankan.',
    },
    {
      title: 'de editor neemt\njouw beeld aan',
      body: 'kies een willekeurige achtergrond en de interface leest het palet. de code blijft leesbaar, rustig, altijd schoon bovenop. de omgeving wordt de jouwe.',
    },
    {
      title: 'hij kijkt naar buiten\nals het moet',
      body: 'staat het antwoord niet in je project, dan zoekt shepherd op het web. zonder account, zonder sleutel. elke zoekopdracht vraagt het eerst, en de bronnen staan direct onder het antwoord.',
    },
  ],
  panels: {
    connected: 'verbonden modellen',
    yourKey: 'jouw sleutel',
    local: 'lokaal',
    nothingMetered: 'niets gemeten. niets naar huis gestuurd.',
    webSearch: 'webzoeken · vraagt eerst',
  },
  privacy: {
    title: 'gratis. privé. van jou.',
    body: 'geen account, geen betaalmuur, geen telemetrie. je projecten zijn niet van ons en we houden er geen gegevens van bij. de modellen die je verbindt antwoorden jou, onder jouw sleutel, niet ons.',
    accounts: 'accounts',
    telemetry: 'telemetrie',
    localModels: 'lokale modellen',
    paywalls: 'betaalmuren',
  },
  cta: { eyebrow: 'als je er klaar voor bent', lineOne: 'kom in de stilte', lineTwo: 'en begin.' },
  download: {
    forPlatform: 'downloaden voor {platform}',
    all: 'alle downloads →',
    none: 'er is nog geen versie van noah voor {platform}.',
    button: 'downloaden',
  },
  downloadPage: {
    title: 'noah downloaden',
    metaTitle: 'noah downloaden voor windows en linux',
    description: 'download noah {version}, de gratis ai-code-editor met shepherd. een installer voor windows 10 en 11, een .deb en een .tar.xz voor linux.',
    meta: 'gratis · geen account · geen telemetrie',
    heading: 'noah downloaden',
    lede: 'De editor en shepherd, zijn agent, in één installatie. Zodra hij opent, voeg je een API-sleutel toe van een aanbieder die je gebruikt, of wijs je hem naar een lokaal model, en begin je.',
    check: 'Controleer je download',
    needs: 'Wat hij nodig heeft',
  },
  english: 'deze pagina is voorlopig in het engels.',
  home: {
    description:
      'noah is een gratis ai-code-editor voor windows en linux. shepherd, zijn agent, leest eerst je hele project. bijna vijftig aanbieders met je eigen sleutel, of lokale modellen.',
  },
}

const pl: Dictionary = {
  nav: { features: 'funkcje', shepherd: 'shepherd', faq: 'pytania', founder: 'założyciel', download: 'pobierz', language: 'język' },
  footer: { by: 'od house of asher', security: 'bezpieczeństwo', terms: 'warunki', source: 'kod źródłowy', stats: 'statystyki' },
  hero: {
    claim: 'pasterz dla twojego kodu.',
    proof: 'czyta cały twój projekt i pisze tak, jakby zawsze w nim mieszkał.',
  },
  features: [
    {
      title: 'najpierw czyta\ncały projekt',
      body: 'zaimportuj repozytorium lub otwórz folder, a shepherd zbada go, zanim napisze jedną linię: architekturę, nazwy, wzorce, które już wybrałeś. potem się ich trzyma, więc jego kod wygląda jak pisany ręką, która zaczęła projekt.',
    },
    {
      title: 'twoje modele.\ntwoje klucze.',
      body: 'przynieś klucz od jednego z prawie pięćdziesięciu dostawców, zachodnich lub chińskich, albo wskaż shepherdowi model działający na twojej maszynie. modele lokalne ogranicza tylko twój sprzęt: 7b lub 405b, ile pomieści pamięć.',
    },
    {
      title: 'edytor przyjmuje\ntwój obraz',
      body: 'ustaw dowolną tapetę, a interfejs odczyta jej paletę. kod pozostaje czytelny, spokojny, zawsze czysto na wierzchu. środowisko staje się twoje.',
    },
    {
      title: 'patrzy na zewnątrz,\ngdy musi',
      body: 'gdy odpowiedzi nie ma w twoim projekcie, shepherd szuka w sieci. bez konta, bez klucza. każde wyszukiwanie najpierw pyta, a źródła są tuż pod odpowiedzią.',
    },
  ],
  panels: {
    connected: 'podłączone modele',
    yourKey: 'twój klucz',
    local: 'lokalny',
    nothingMetered: 'nic nie liczone. nic nie wysyłane.',
    webSearch: 'wyszukiwanie w sieci · najpierw pyta',
  },
  privacy: {
    title: 'darmowy. prywatny. twój.',
    body: 'bez konta, bez paywalla, bez telemetrii. twoje projekty nie są nasze i nie prowadzimy o nich zapisków. modele, które podłączasz, odpowiadają tobie, pod twoim kluczem, nie nam.',
    accounts: 'kont',
    telemetry: 'telemetrii',
    localModels: 'modeli lokalnych',
    paywalls: 'paywalli',
  },
  cta: { eyebrow: 'kiedy będziesz gotowy', lineOne: 'przyjdź do ciszy', lineTwo: 'i zacznij.' },
  download: {
    forPlatform: 'pobierz na {platform}',
    all: 'wszystkie pliki →',
    none: 'nie ma jeszcze wersji noah na {platform}.',
    button: 'pobierz',
  },
  downloadPage: {
    title: 'Pobierz noah',
    metaTitle: 'pobierz noah na windows i linux',
    description: 'pobierz noah {version}, darmowy edytor kodu z ai i shepherdem. instalator na windows 10 i 11, .deb i .tar.xz na linux.',
    meta: 'darmowy · bez konta · bez telemetrii',
    heading: 'Pobierz noah',
    lede: 'Edytor i shepherd, jego agent, w jednej instalacji. Po otwarciu dodaj klucz API dowolnego dostawcy, z którego korzystasz, albo wskaż model lokalny, i zacznij.',
    check: 'Sprawdź pobrany plik',
    needs: 'Czego potrzebuje',
  },
  english: 'ta strona jest na razie po angielsku.',
  home: {
    description:
      'noah to darmowy edytor kodu z ai na windows i linux. shepherd, jego agent, najpierw czyta cały twój projekt. prawie pięćdziesięciu dostawców na twoim kluczu albo modele lokalne.',
  },
}

const tr: Dictionary = {
  nav: { features: 'özellikler', shepherd: 'shepherd', faq: 'sorular', founder: 'kurucu', download: 'indir', language: 'dil' },
  footer: { by: 'house of asher yapımı', security: 'güvenlik', terms: 'koşullar', source: 'kaynak', stats: 'istatistik' },
  hero: {
    claim: 'kodun için bir çoban.',
    proof: 'tüm projeni okur ve hep oradaymış gibi yazar.',
  },
  features: [
    {
      title: 'önce tüm projeyi\nokur',
      body: 'bir depo içe aktar ya da bir klasör aç; shepherd tek satır yazmadan önce onu inceler: mimariyi, adlandırmayı, zaten seçtiğin kalıpları. sonra onlara uyar, böylece kodu projeyi başlatan elin yazısı gibi okunur.',
    },
    {
      title: 'senin modellerin.\nsenin anahtarların.',
      body: 'batılı ya da çinli yaklaşık elli sağlayıcıdan birinin anahtarını getir, ya da shepherd’ı kendi makinende çalışan bir modele yönlendir. yerel modelleri yalnızca donanımın sınırlar: 7b ya da 405b, belleğine ne sığarsa.',
    },
    {
      title: 'editör senin\ngörüntünü alır',
      body: 'herhangi bir duvar kağıdı seç, arayüz paletini okur. kod okunaklı ve sakin kalır, her zaman temizce üstte durur. ortam senin olur.',
    },
    {
      title: 'gerektiğinde\ndışarı bakar',
      body: 'cevap projende yoksa shepherd webde arar. hesap yok, anahtar yok. her arama önce sana sorar ve kaynaklar cevabın hemen altında durur.',
    },
  ],
  panels: {
    connected: 'bağlı modeller',
    yourKey: 'senin anahtarın',
    local: 'yerel',
    nothingMetered: 'hiçbir şey ölçülmez. hiçbir şey eve gönderilmez.',
    webSearch: 'web araması · önce sorar',
  },
  privacy: {
    title: 'ücretsiz. özel. senin.',
    body: 'hesap yok, ödeme duvarı yok, telemetri yok. projelerin bize ait değil, kayıt da tutmuyoruz. bağladığın modeller senin anahtarınla sana cevap verir, bize değil.',
    accounts: 'hesap',
    telemetry: 'telemetri',
    localModels: 'yerel model',
    paywalls: 'ödeme duvarı',
  },
  cta: { eyebrow: 'hazır olduğunda', lineOne: 'sessizliğe gel', lineTwo: 've başla.' },
  download: {
    forPlatform: '{platform} için indir',
    all: 'tüm indirmeler →',
    none: 'noah’ın henüz {platform} sürümü yok.',
    button: 'indir',
  },
  downloadPage: {
    title: 'noah’ı indir',
    metaTitle: 'windows ve linux için noah’ı indir',
    description: 'shepherd’lı ücretsiz yapay zekâ kod editörü noah {version} sürümünü indir. windows 10 ve 11 için bir kurulum dosyası, linux için .deb ve .tar.xz.',
    meta: 'ücretsiz · hesap yok · telemetri yok',
    heading: 'noah’ı indir',
    lede: 'Editör ve ajanı shepherd tek kurulumda. Açıldığında kullandığın herhangi bir sağlayıcının API anahtarını ekle ya da yerel bir modele yönlendir ve başla.',
    check: 'İndirdiğin dosyayı doğrula',
    needs: 'Neye ihtiyacı var',
  },
  english: 'bu sayfa şimdilik ingilizce.',
  home: {
    description:
      'noah, windows ve linux için ücretsiz bir yapay zekâ kod editörüdür. ajanı shepherd önce tüm projeni okur. kendi anahtarınla yaklaşık elli sağlayıcı ya da yerel modeller.',
  },
}

const ru: Dictionary = {
  nav: { features: 'возможности', shepherd: 'shepherd', faq: 'вопросы', founder: 'основатель', download: 'скачать', language: 'язык' },
  footer: { by: 'от house of asher', security: 'безопасность', terms: 'условия', source: 'исходники', stats: 'статистика' },
  hero: {
    claim: 'пастух для вашего кода.',
    proof: 'он читает весь проект и пишет так, будто всегда в нём жил.',
  },
  features: [
    {
      title: 'сначала он читает\nвесь проект',
      body: 'импортируйте репозиторий или откройте папку, и shepherd изучит её, прежде чем написать строку: архитектуру, имена, шаблоны, которые вы уже выбрали. потом он следует им, и его код читается как рука, начавшая проект.',
    },
    {
      title: 'ваши модели.\nваши ключи.',
      body: 'принесите ключ любого из почти пятидесяти провайдеров, западных или китайских, или направьте shepherd на модель на вашей машине. локальные модели ограничены только железом: 7b или 405b, сколько вместит память.',
    },
    {
      title: 'редактор принимает\nваш образ',
      body: 'поставьте любые обои, и интерфейс прочитает их палитру. код остаётся читаемым, спокойным, всегда чисто сверху. среда становится вашей.',
    },
    {
      title: 'он смотрит наружу,\nкогда нужно',
      body: 'если ответа нет в проекте, shepherd ищет в сети. без аккаунта, без ключа. каждый поиск сначала спрашивает вас, а источники стоят прямо под ответом.',
    },
  ],
  panels: {
    connected: 'подключённые модели',
    yourKey: 'ваш ключ',
    local: 'локально',
    nothingMetered: 'ничего не считается. ничего не отправляется.',
    webSearch: 'поиск в сети · сначала спрашивает',
  },
  privacy: {
    title: 'бесплатно. приватно. ваше.',
    body: 'без аккаунта, без платного доступа, без телеметрии. ваши проекты не наши, и мы не ведём о них записей. модели, которые вы подключаете, отвечают вам, под вашим ключом, а не нам.',
    accounts: 'аккаунтов',
    telemetry: 'телеметрии',
    localModels: 'локальных моделей',
    paywalls: 'платных стен',
  },
  cta: { eyebrow: 'когда будете готовы', lineOne: 'придите в тишину', lineTwo: 'и начните.' },
  download: {
    forPlatform: 'скачать для {platform}',
    all: 'все загрузки →',
    none: 'версии noah для {platform} пока нет.',
    button: 'скачать',
  },
  downloadPage: {
    title: 'Скачать noah',
    metaTitle: 'скачать noah для windows и linux',
    description: 'скачайте noah {version}, бесплатный ии-редактор кода с shepherd. установщик для windows 10 и 11, .deb и .tar.xz для linux.',
    meta: 'бесплатно · без аккаунта · без телеметрии',
    heading: 'Скачать noah',
    lede: 'Редактор и shepherd, его агент, в одной установке. Когда он откроется, добавьте API-ключ любого провайдера, которым пользуетесь, или укажите локальную модель, и начинайте.',
    check: 'Проверьте загрузку',
    needs: 'Что ему нужно',
  },
  english: 'эта страница пока на английском.',
  home: {
    description:
      'noah — бесплатный ии-редактор кода для windows и linux. shepherd, его агент, сначала читает весь ваш проект. почти пятьдесят провайдеров по вашему ключу или локальные модели.',
  },
}

const uk: Dictionary = {
  nav: { features: 'можливості', shepherd: 'shepherd', faq: 'питання', founder: 'засновник', download: 'завантажити', language: 'мова' },
  footer: { by: 'від house of asher', security: 'безпека', terms: 'умови', source: 'вихідний код', stats: 'статистика' },
  hero: {
    claim: 'пастух для вашого коду.',
    proof: 'він читає весь проєкт і пише так, ніби завжди в ньому жив.',
  },
  features: [
    {
      title: 'спершу він читає\nувесь проєкт',
      body: 'імпортуйте репозиторій або відкрийте теку, і shepherd вивчить її, перш ніж написати рядок: архітектуру, імена, шаблони, які ви вже обрали. потім він дотримується їх, і його код читається як рука, що почала проєкт.',
    },
    {
      title: 'ваші моделі.\nваші ключі.',
      body: 'принесіть ключ будь-якого з майже п’ятдесяти провайдерів, західних чи китайських, або спрямуйте shepherd на модель на вашій машині. локальні моделі обмежує лише залізо: 7b чи 405b, скільки вмістить пам’ять.',
    },
    {
      title: 'редактор приймає\nваш образ',
      body: 'поставте будь-які шпалери, і інтерфейс прочитає їхню палітру. код лишається читабельним, спокійним, завжди чисто зверху. середовище стає вашим.',
    },
    {
      title: 'він дивиться назовні,\nколи треба',
      body: 'якщо відповіді немає у проєкті, shepherd шукає в мережі. без облікового запису, без ключа. кожен пошук спершу питає вас, а джерела стоять одразу під відповіддю.',
    },
  ],
  panels: {
    connected: 'під’єднані моделі',
    yourKey: 'ваш ключ',
    local: 'локально',
    nothingMetered: 'нічого не лічиться. нічого не надсилається.',
    webSearch: 'пошук у мережі · спершу питає',
  },
  privacy: {
    title: 'безкоштовно. приватно. ваше.',
    body: 'без облікового запису, без платного доступу, без телеметрії. ваші проєкти не наші, і ми не ведемо про них записів. моделі, які ви під’єднуєте, відповідають вам, під вашим ключем, а не нам.',
    accounts: 'облікових записів',
    telemetry: 'телеметрії',
    localModels: 'локальних моделей',
    paywalls: 'платних стін',
  },
  cta: { eyebrow: 'коли будете готові', lineOne: 'прийдіть у тишу', lineTwo: 'і почніть.' },
  download: {
    forPlatform: 'завантажити для {platform}',
    all: 'усі завантаження →',
    none: 'версії noah для {platform} поки немає.',
    button: 'завантажити',
  },
  downloadPage: {
    title: 'Завантажити noah',
    metaTitle: 'завантажити noah для windows і linux',
    description: 'завантажте noah {version}, безкоштовний ші-редактор коду з shepherd. інсталятор для windows 10 і 11, .deb і .tar.xz для linux.',
    meta: 'безкоштовно · без облікового запису · без телеметрії',
    heading: 'Завантажити noah',
    lede: 'Редактор і shepherd, його агент, в одній установці. Коли він відкриється, додайте API-ключ будь-якого провайдера, яким користуєтесь, або вкажіть локальну модель, і починайте.',
    check: 'Перевірте завантаження',
    needs: 'Що йому потрібно',
  },
  english: 'ця сторінка поки що англійською.',
  home: {
    description:
      'noah — безкоштовний ші-редактор коду для windows і linux. shepherd, його агент, спершу читає весь ваш проєкт. майже п’ятдесят провайдерів за вашим ключем або локальні моделі.',
  },
}

const ar: Dictionary = {
  nav: { features: 'الميزات', shepherd: 'shepherd', faq: 'أسئلة', founder: 'المؤسس', download: 'تنزيل', language: 'اللغة' },
  footer: { by: 'من house of asher', security: 'الأمان', terms: 'الشروط', source: 'المصدر', stats: 'إحصاءات' },
  hero: {
    claim: 'راعٍ لشيفرتك.',
    proof: 'يقرأ مشروعك كله ويكتب كأنه عاش فيه منذ البداية.',
  },
  features: [
    {
      title: 'يقرأ المشروع\nكله أولًا',
      body: 'استورد مستودعًا أو افتح مجلدًا، فيدرسه shepherd قبل أن يكتب سطرًا واحدًا: البنية، والتسمية، والأنماط التي اخترتها. ثم يلتزم بها، فتُقرأ شيفرته كأنها بخط اليد التي بدأت المشروع.',
    },
    {
      title: 'نماذجك.\nمفاتيحك.',
      body: 'أحضر مفتاحًا من أي مزود من نحو خمسين مزودًا، غربيًا أو صينيًا، أو وجّه shepherd إلى نموذج يعمل على جهازك. النماذج المحلية لا يحدّها إلا عتادك: 7b أو 405b، ما تتسع له ذاكرتك.',
    },
    {
      title: 'المحرر يأخذ\nصورتك',
      body: 'ضع أي خلفية فتقرأ الواجهة لوحة ألوانها. تبقى الشيفرة مقروءة وهادئة، دائمًا نظيفة في المقدمة. تصبح البيئة بيئتك.',
    },
    {
      title: 'ينظر إلى الخارج\nعند الحاجة',
      body: 'حين لا يكون الجواب في مشروعك، يبحث shepherd في الويب. بلا حساب ولا مفتاح. كل بحث يستأذنك أولًا، والمصادر تقف مباشرة تحت الجواب.',
    },
  ],
  panels: {
    connected: 'النماذج المتصلة',
    yourKey: 'مفتاحك',
    local: 'محلي',
    nothingMetered: 'لا شيء يُحتسب. لا شيء يُرسل.',
    webSearch: 'بحث الويب · يستأذن أولًا',
  },
  privacy: {
    title: 'مجاني. خاص. لك.',
    body: 'بلا حساب، بلا جدار دفع، بلا قياس عن بُعد. مشاريعك ليست ملكنا ولا نحتفظ بسجلات عنها. النماذج التي تصلها تجيبك أنت، بمفتاحك، لا نحن.',
    accounts: 'حسابات',
    telemetry: 'قياس عن بُعد',
    localModels: 'نماذج محلية',
    paywalls: 'جدران دفع',
  },
  cta: { eyebrow: 'حين تكون مستعدًا', lineOne: 'تعال إلى الهدوء', lineTwo: 'وابدأ.' },
  download: {
    forPlatform: 'تنزيل لنظام {platform}',
    all: 'كل التنزيلات ←',
    none: 'لا توجد نسخة من noah لنظام {platform} بعد.',
    button: 'تنزيل',
  },
  downloadPage: {
    title: 'تنزيل noah',
    metaTitle: 'تنزيل noah لويندوز ولينكس',
    description: 'نزّل noah {version}، محرر الشيفرة المجاني بالذكاء الاصطناعي مع shepherd. مثبّت لويندوز 10 و11، وحزمة .deb وأرشيف .tar.xz للينكس.',
    meta: 'مجاني · بلا حساب · بلا قياس عن بُعد',
    heading: 'تنزيل noah',
    lede: 'المحرر وshepherd، وكيله، في تثبيت واحد. بعد أن يفتح، أضف مفتاح API من أي مزود تستخدمه، أو وجّهه إلى نموذج محلي، وابدأ.',
    check: 'تحقق من التنزيل',
    needs: 'ما يحتاجه',
  },
  english: 'هذه الصفحة بالإنجليزية حاليًا.',
  home: {
    description:
      'noah محرر شيفرة مجاني بالذكاء الاصطناعي لويندوز ولينكس. shepherd، وكيله، يقرأ مشروعك كله أولًا. نحو خمسين مزودًا بمفتاحك، أو نماذج محلية.',
  },
}

const hi: Dictionary = {
  nav: { features: 'खूबियाँ', shepherd: 'shepherd', faq: 'सवाल', founder: 'संस्थापक', download: 'डाउनलोड', language: 'भाषा' },
  footer: { by: 'house of asher की ओर से', security: 'सुरक्षा', terms: 'शर्तें', source: 'सोर्स', stats: 'आँकड़े' },
  hero: {
    claim: 'आपके कोड का एक चरवाहा।',
    proof: 'यह आपका पूरा प्रोजेक्ट पढ़ता है और ऐसे लिखता है जैसे हमेशा से वहीं रहा हो।',
  },
  features: [
    {
      title: 'पहले पूरा प्रोजेक्ट\nपढ़ता है',
      body: 'कोई रिपो इम्पोर्ट करें या फ़ोल्डर खोलें, और shepherd एक लाइन लिखने से पहले उसे समझता है: आर्किटेक्चर, नामकरण, वे पैटर्न जो आपने पहले से चुने हैं। फिर उन्हीं पर चलता है, ताकि उसका कोड उसी हाथ का लगे जिसने प्रोजेक्ट शुरू किया था।',
    },
    {
      title: 'आपके मॉडल।\nआपकी कुंजियाँ।',
      body: 'पश्चिमी या चीनी, लगभग पचास प्रदाताओं में से किसी की भी कुंजी लाएँ, या shepherd को अपनी मशीन पर चल रहे मॉडल की ओर मोड़ें। लोकल मॉडल की सीमा सिर्फ़ आपका हार्डवेयर है: 7b हो या 405b, जितना आपकी मेमोरी में समाए।',
    },
    {
      title: 'एडिटर आपकी\nछवि ले लेता है',
      body: 'कोई भी वॉलपेपर लगाएँ और इंटरफ़ेस उसका रंग-पैलेट पढ़ लेता है। कोड पढ़ने योग्य और शांत रहता है, हमेशा साफ़ ऊपर बैठा हुआ। माहौल आपका हो जाता है।',
    },
    {
      title: 'ज़रूरत पड़ने पर\nबाहर देखता है',
      body: 'जब जवाब आपके प्रोजेक्ट में न हो, shepherd वेब पर खोजता है। न खाता, न कुंजी। हर खोज पहले आपसे पूछती है, और स्रोत जवाब के ठीक नीचे रहते हैं।',
    },
  ],
  panels: {
    connected: 'जुड़े हुए मॉडल',
    yourKey: 'आपकी कुंजी',
    local: 'लोकल',
    nothingMetered: 'कुछ नहीं गिना जाता। कुछ नहीं भेजा जाता।',
    webSearch: 'वेब खोज · पहले पूछता है',
  },
  privacy: {
    title: 'मुफ़्त। निजी। आपका।',
    body: 'न खाता, न पेवॉल, न टेलीमेट्री। आपके प्रोजेक्ट हमारे नहीं हैं और हम उनका कोई रिकॉर्ड नहीं रखते। जो मॉडल आप जोड़ते हैं वे आपकी कुंजी से आपको जवाब देते हैं, हमें नहीं।',
    accounts: 'खाते',
    telemetry: 'टेलीमेट्री',
    localModels: 'लोकल मॉडल',
    paywalls: 'पेवॉल',
  },
  cta: { eyebrow: 'जब आप तैयार हों', lineOne: 'शांति में आइए', lineTwo: 'और शुरू कीजिए।' },
  download: {
    forPlatform: '{platform} के लिए डाउनलोड',
    all: 'सभी डाउनलोड →',
    none: '{platform} के लिए noah का संस्करण अभी नहीं है।',
    button: 'डाउनलोड',
  },
  downloadPage: {
    title: 'noah डाउनलोड करें',
    metaTitle: 'windows और linux के लिए noah डाउनलोड करें',
    description: 'shepherd के साथ मुफ़्त एआई कोड एडिटर noah {version} डाउनलोड करें। windows 10 और 11 के लिए इंस्टॉलर, linux के लिए .deb और .tar.xz।',
    meta: 'मुफ़्त · कोई खाता नहीं · कोई टेलीमेट्री नहीं',
    heading: 'noah डाउनलोड करें',
    lede: 'एडिटर और उसका एजेंट shepherd, एक ही इंस्टॉल में। खुलने के बाद, अपने किसी भी प्रदाता की API कुंजी जोड़ें, या इसे किसी लोकल मॉडल की ओर मोड़ें, और शुरू करें।',
    check: 'अपना डाउनलोड जाँचें',
    needs: 'इसे क्या चाहिए',
  },
  english: 'यह पेज अभी अंग्रेज़ी में है।',
  home: {
    description:
      'noah windows और linux के लिए एक मुफ़्त एआई कोड एडिटर है। इसका एजेंट shepherd पहले आपका पूरा प्रोजेक्ट पढ़ता है। आपकी अपनी कुंजी पर लगभग पचास प्रदाता, या लोकल मॉडल।',
  },
}

const ja: Dictionary = {
  nav: { features: '機能', shepherd: 'shepherd', faq: 'よくある質問', founder: '創業者', download: 'ダウンロード', language: '言語' },
  footer: { by: 'house of asher より', security: 'セキュリティ', terms: '利用規約', source: 'ソース', stats: '統計' },
  hero: {
    claim: 'あなたのコードの羊飼い。',
    proof: 'プロジェクト全体を読み、最初からそこにいたかのように書く。',
  },
  features: [
    {
      title: 'まずプロジェクト\n全体を読む',
      body: 'リポジトリを取り込むか、フォルダを開くと、shepherd は一行書く前にそれを学びます。構成、命名、あなたがすでに選んだパターン。そしてそれに合わせるので、書かれるコードはプロジェクトを始めた手のように読めます。',
    },
    {
      title: 'あなたのモデル。\nあなたのキー。',
      body: '欧米や中国の約五十のプロバイダーのどれかのキーを持ち込むか、自分のマシンで動くモデルを shepherd に指定できます。ローカルモデルの上限はハードウェアだけ。7b でも 405b でも、メモリに収まる限り。',
    },
    {
      title: 'エディタが\nあなたの色になる',
      body: '好きな壁紙を設定すると、インターフェースがそのパレットを読み取ります。コードは読みやすく、静かに、いつも一番上に澄んで座ります。環境はあなたのものになります。',
    },
    {
      title: '必要なときは\n外を見る',
      body: '答えがプロジェクトの中にないとき、shepherd はウェブを検索します。アカウントもキーも不要。検索のたびに先にあなたへ尋ね、出典は答えのすぐ下に並びます。',
    },
  ],
  panels: {
    connected: '接続中のモデル',
    yourKey: 'あなたのキー',
    local: 'ローカル',
    nothingMetered: '何も計測しない。何も送信しない。',
    webSearch: 'ウェブ検索 · 先に尋ねる',
  },
  privacy: {
    title: '無料。プライベート。あなたのもの。',
    body: 'アカウントなし、有料の壁なし、テレメトリなし。あなたのプロジェクトは私たちのものではなく、記録も残しません。接続したモデルはあなたのキーであなたに応えます。私たちにではなく。',
    accounts: 'アカウント',
    telemetry: 'テレメトリ',
    localModels: 'ローカルモデル',
    paywalls: '有料の壁',
  },
  cta: { eyebrow: '準備ができたら', lineOne: '静けさへ来て', lineTwo: '始めよう。' },
  download: {
    forPlatform: '{platform} 版をダウンロード',
    all: 'すべてのダウンロード →',
    none: '{platform} 版の noah はまだありません。',
    button: 'ダウンロード',
  },
  downloadPage: {
    title: 'noah をダウンロード',
    metaTitle: 'windows と linux 向け noah をダウンロード',
    description: 'shepherd を備えた無料の ai コードエディタ noah {version} をダウンロード。windows 10 と 11 向けのインストーラ、linux 向けの .deb と .tar.xz。',
    meta: '無料 · アカウント不要 · テレメトリなし',
    heading: 'noah をダウンロード',
    lede: 'エディタとそのエージェント shepherd を一度のインストールで。開いたら、使っているプロバイダーの API キーを追加するか、ローカルモデルを指定して、始めてください。',
    check: 'ダウンロードを確認する',
    needs: '必要なもの',
  },
  english: 'このページは今のところ英語です。',
  home: {
    description:
      'noah は windows と linux 向けの無料 ai コードエディタです。エージェントの shepherd はまずプロジェクト全体を読みます。あなた自身のキーで約五十のプロバイダー、またはローカルモデル。',
  },
}

const ko: Dictionary = {
  nav: { features: '기능', shepherd: 'shepherd', faq: '자주 묻는 질문', founder: '창립자', download: '다운로드', language: '언어' },
  footer: { by: 'house of asher 제작', security: '보안', terms: '약관', source: '소스', stats: '통계' },
  hero: {
    claim: '당신의 코드를 위한 목자.',
    proof: '프로젝트 전체를 읽고, 처음부터 거기 있었던 것처럼 씁니다.',
  },
  features: [
    {
      title: '먼저 프로젝트\n전체를 읽습니다',
      body: '저장소를 가져오거나 폴더를 열면 shepherd는 한 줄을 쓰기 전에 그것을 공부합니다. 구조, 이름 짓기, 당신이 이미 고른 패턴. 그리고 거기에 맞추기 때문에 그 코드는 프로젝트를 시작한 손처럼 읽힙니다.',
    },
    {
      title: '당신의 모델.\n당신의 키.',
      body: '서양이든 중국이든 약 오십 개 제공자 중 어디서든 키를 가져오거나, 당신의 컴퓨터에서 도는 모델을 shepherd에 연결하세요. 로컬 모델의 한계는 하드웨어뿐입니다. 7b든 405b든 메모리에 들어가는 만큼.',
    },
    {
      title: '에디터가\n당신의 모습이 됩니다',
      body: '어떤 배경화면이든 설정하면 인터페이스가 그 팔레트를 읽습니다. 코드는 읽기 쉽고 차분하게, 언제나 맨 위에 깨끗이 자리합니다. 환경이 당신의 것이 됩니다.',
    },
    {
      title: '필요할 때는\n밖을 봅니다',
      body: '답이 프로젝트 안에 없으면 shepherd는 웹을 검색합니다. 계정도 키도 없이. 모든 검색은 먼저 당신에게 묻고, 출처는 답 바로 아래에 놓입니다.',
    },
  ],
  panels: {
    connected: '연결된 모델',
    yourKey: '당신의 키',
    local: '로컬',
    nothingMetered: '아무것도 세지 않습니다. 아무것도 보내지 않습니다.',
    webSearch: '웹 검색 · 먼저 묻습니다',
  },
  privacy: {
    title: '무료. 비공개. 당신의 것.',
    body: '계정 없음, 유료 장벽 없음, 원격 측정 없음. 당신의 프로젝트는 우리 것이 아니며 기록도 남기지 않습니다. 연결한 모델은 당신의 키로 당신에게 답합니다. 우리에게가 아니라.',
    accounts: '계정',
    telemetry: '원격 측정',
    localModels: '로컬 모델',
    paywalls: '유료 장벽',
  },
  cta: { eyebrow: '준비가 되면', lineOne: '고요함으로 와서', lineTwo: '시작하세요.' },
  download: {
    forPlatform: '{platform}용 다운로드',
    all: '모든 다운로드 →',
    none: '{platform}용 noah는 아직 없습니다.',
    button: '다운로드',
  },
  downloadPage: {
    title: 'noah 다운로드',
    metaTitle: 'windows와 linux용 noah 다운로드',
    description: 'shepherd가 있는 무료 ai 코드 에디터 noah {version}을 다운로드하세요. windows 10과 11용 설치 파일, linux용 .deb와 .tar.xz.',
    meta: '무료 · 계정 없음 · 원격 측정 없음',
    heading: 'noah 다운로드',
    lede: '에디터와 그 에이전트 shepherd를 한 번의 설치로. 열리면 사용하는 제공자의 API 키를 추가하거나 로컬 모델을 연결하고 시작하세요.',
    check: '다운로드 확인하기',
    needs: '필요한 것',
  },
  english: '이 페이지는 아직 영어로 되어 있습니다.',
  home: {
    description:
      'noah는 windows와 linux용 무료 ai 코드 에디터입니다. 에이전트 shepherd가 먼저 프로젝트 전체를 읽습니다. 당신의 키로 약 오십 개 제공자, 또는 로컬 모델.',
  },
}

const zh: Dictionary = {
  nav: { features: '功能', shepherd: 'shepherd', faq: '常见问题', founder: '创始人', download: '下载', language: '语言' },
  footer: { by: 'house of asher 出品', security: '安全', terms: '条款', source: '源代码', stats: '统计' },
  hero: {
    claim: '你的代码的牧羊人。',
    proof: '它读完你的整个项目，写出来的代码就像一直住在里面。',
  },
  features: [
    {
      title: '先读完\n整个项目',
      body: '导入一个仓库或打开一个文件夹，shepherd 会在写下一行之前先研究它：架构、命名、你已经选定的模式。然后照着来，所以它的代码读起来就像最初动手的那只手。',
    },
    {
      title: '你的模型。\n你的密钥。',
      body: '带上近五十家提供商中任何一家的密钥，无论欧美还是中国，或者让 shepherd 使用你自己机器上运行的模型。本地模型只受你的硬件限制：7b 或 405b，内存装得下就行。',
    },
    {
      title: '编辑器随你\n的画面而变',
      body: '设置任意壁纸，界面会读取它的色板。代码始终清晰、平静，干干净净地浮在最上层。环境变成你的。',
    },
    {
      title: '需要时\n向外看',
      body: '当答案不在你的项目里，shepherd 会搜索网络。不需要账号，不需要密钥。每次搜索都先问你，来源就排在答案下面。',
    },
  ],
  panels: {
    connected: '已连接的模型',
    yourKey: '你的密钥',
    local: '本地',
    nothingMetered: '不计量，不回传。',
    webSearch: '网络搜索 · 先问你',
  },
  privacy: {
    title: '免费。私密。属于你。',
    body: '没有账号，没有付费墙，没有遥测。你的项目不属于我们，我们也不留记录。你连接的模型用你的密钥回答你，而不是我们。',
    accounts: '账号',
    telemetry: '遥测',
    localModels: '本地模型',
    paywalls: '付费墙',
  },
  cta: { eyebrow: '当你准备好了', lineOne: '来到安静之处', lineTwo: '然后开始。' },
  download: {
    forPlatform: '下载 {platform} 版',
    all: '全部下载 →',
    none: '还没有 {platform} 版的 noah。',
    button: '下载',
  },
  downloadPage: {
    title: '下载 noah',
    metaTitle: '下载 windows 和 linux 版 noah',
    description: '下载 noah {version}，内置 shepherd 的免费 ai 代码编辑器。windows 10 和 11 的安装程序，linux 的 .deb 和 .tar.xz。',
    meta: '免费 · 无需账号 · 无遥测',
    heading: '下载 noah',
    lede: '编辑器和它的代理 shepherd，一次安装。打开后，添加你所用任意提供商的 API 密钥，或指向一个本地模型，然后开始。',
    check: '校验你的下载',
    needs: '运行要求',
  },
  english: '此页面暂时只有英文。',
  home: {
    description:
      'noah 是一款面向 windows 和 linux 的免费 ai 代码编辑器。它的代理 shepherd 会先读完你的整个项目。用你自己的密钥接入近五十家提供商，或使用本地模型。',
  },
}

export const DICTIONARIES: Record<Language, Dictionary> = { en, es, pt, fr, de, it, nl, pl, tr, ru, uk, ar, hi, ja, ko, zh }

export function dictionary(language: Language): Dictionary {
  return DICTIONARIES[language]
}

/** Fills `{platform}`-style holes in a translated line. */
export function fill(line: string, values: Record<string, string>): string {
  return line.replace(/\{(\w+)\}/g, (match, key: string) => values[key] ?? match)
}
