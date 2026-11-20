import type { Metadata } from 'next'
import JsonLd from '@/components/JsonLd'
import Shell from '@/components/landing/Shell'
import { RELEASE_VERSION, type Installer } from '@/lib/site'
import { availableInstallers } from '@/lib/installers'
import { breadcrumbs, pageMetadata } from '@/lib/seo'
import { DEFAULT_LANGUAGE, dictionary, fill } from '@/lib/i18n'
import { currentLanguage } from '@/lib/language'
import { SCREEN_IMAGES } from '@/lib/screens'
import './shots.css'

export function generateMetadata(): Metadata {
  const language = currentLanguage()
  const t = dictionary(language).downloadPage
  return pageMetadata({
    path: '/download',
    title: t.metaTitle,
    description: fill(t.description, { version: RELEASE_VERSION }),
    language,
    translated: true,
  })
}

const SCREENS = [
  { file: 'editor', caption: 'the editor, over your wallpaper', alt: 'noah editor showing a Rust file' },
  { file: 'shepherd', caption: 'shepherd: any provider, or a local model', alt: 'shepherd panel beside the code' },
]

function Steps({ installer }: { installer: Installer }) {
  switch (installer.platform) {
    case 'windows':
      return (
        <>
          <ol>
            <li>
              If your browser says the file &ldquo;may be dangerous&rdquo; or &ldquo;isn&rsquo;t commonly
              downloaded&rdquo;, open the downloads list and choose <strong>Keep</strong>. Browsers warn about
              any new, unsigned program; the SHA-256 below lets you confirm the file is the one published here.
            </li>
            <li>Open the downloaded <code>{installer.file}</code>.</li>
            <li>
              If Windows shows &ldquo;Windows protected your PC&rdquo;, choose <strong>More info</strong>, then{' '}
              <strong>Run anyway</strong>. The installer is not code-signed yet, so SmartScreen does not
              recognise it.
            </li>
            <li>
              It installs for your user only (no administrator prompt) into{' '}
              <code>%LOCALAPPDATA%\Programs\noah</code>, adds Start menu and desktop shortcuts, and opens noah.
            </li>
          </ol>
          <p>To remove it: Settings → Apps → Installed apps → noah → Uninstall.</p>
        </>
      )
    case 'linux':
      return installer.file.endsWith('.deb') ? (
        <ol>
          <li>Open the downloaded <code>{installer.file}</code>; your software center installs it.</li>
          <li>
            Or from a terminal: <code>sudo apt install ./{installer.file}</code>
          </li>
          <li>Start noah from your app launcher, or run <code>noah</code>.</li>
        </ol>
      ) : (
        <ol>
          <li>
            Extract it: <code>tar -xf {installer.file}</code>
          </li>
          <li>
            Install for your user: <code>./noah.app/install.sh</code>
          </li>
          <li>Start noah from your app launcher, or run <code>noah</code>. Remove it with <code>~/.local/noah.app/install.sh --uninstall</code>.</li>
        </ol>
      )
    case 'mac':
      return (
        <>
          <ol>
            <li>Open the downloaded <code>{installer.file}</code> and drag <strong>noah</strong> into <strong>Applications</strong>.</li>
            <li>
              The first time, macOS may say the app &ldquo;cannot be opened because Apple cannot check it&rdquo;:
              noah is not notarized yet. Right-click (or Control-click) <strong>noah</strong> in Applications and
              choose <strong>Open</strong>, then <strong>Open</strong> again. macOS remembers the choice.
            </li>
            <li>If that option does not appear, open <strong>System Settings → Privacy &amp; Security</strong> and choose <strong>Open Anyway</strong> next to noah.</li>
          </ol>
          <p>To remove it: drag noah from Applications to the Trash.</p>
        </>
      )
  }
}

export default function DownloadPage() {
  const installers = availableInstallers()
  const hasLinux = installers.some((installer) => installer.platform === 'linux')
  const hasMac = installers.some((installer) => installer.platform === 'mac')
  const language = currentLanguage()
  const t = dictionary(language)

  return (
    <Shell>
      <JsonLd data={breadcrumbs([{ name: 'download', path: '/download' }])} />
      <main className="l-doc">
        <div className="l-doc-card">
          <p className="l-doc-meta">{t.downloadPage.meta}</p>
          <h1>{t.downloadPage.heading}</h1>
          <p className="l-doc-lede">{t.downloadPage.lede}</p>
          {language !== DEFAULT_LANGUAGE && <p className="l-english-note">{t.english}</p>}

          {installers.map((installer) => (
            <section key={installer.file} className="l-installer">
              <div className="l-installer-head">
                <div>
                  <div className="l-installer-name">{installer.label}</div>
                  <div className="l-installer-meta">
                    {installer.format} · {installer.size}
                  </div>
                </div>
                <a className="l-btn-primary" href={`/downloads/${installer.file}`} download>
                  {t.download.button}
                </a>
              </div>
              <Steps installer={installer} />
              <p className="l-installer-sum">
                SHA-256: <code>{installer.sha256}</code>
              </p>
            </section>
          ))}

          {!hasMac && (
            <>
              <h2>macOS{hasLinux ? '' : ' and Linux'}</h2>
              <p>
                There is no {hasLinux ? 'macOS build' : 'macOS or Linux build'} yet. When there is, it is listed here with its checksum.
              </p>
            </>
          )}

          <h2>What you get</h2>
          <div className="l-shots">
            {SCREENS.map((screen) => (
              <figure key={screen.file} className="l-shot">
                <img src={SCREEN_IMAGES[screen.file]} alt={screen.alt} width={640} height={407} loading="lazy" decoding="async" />
                <figcaption>{screen.caption}</figcaption>
              </figure>
            ))}
          </div>
          <p>
            The source code is on{' '}
            <a href="https://github.com/iamaureon74/aureon.noah.ide">GitHub</a>. noah is free; if it helps you, a bitcoin
            donation keeps it going: <code>bc1qwhu7xz88spa56mfvmg0zhtwlr4sny0qel2k2t8</code>
          </p>

          <h2>{t.downloadPage.check}</h2>
          <p>
            Compare the file&apos;s SHA-256 with the value above. On Windows, in PowerShell:{' '}
            <code>Get-FileHash .\noah-windows-x86_64.exe</code>. On Linux: <code>sha256sum &lt;file&gt;</code>. On a Mac: <code>shasum -a 256 &lt;file&gt;</code>.
          </p>

          <h2>{t.downloadPage.needs}</h2>
          <ul>
            <li>Windows 10 (version 1607 or later) or Windows 11, 64-bit, with a DirectX 11 graphics driver.</li>
            <li>Linux on x86-64 with a Vulkan graphics driver.</li>
            {hasMac && <li>macOS 10.15 or later, on Apple Silicon or Intel.</li>}
            <li>An internet connection for hosted models and web search. Local models run offline.</li>
          </ul>
        </div>
      </main>
    </Shell>
  )
}
