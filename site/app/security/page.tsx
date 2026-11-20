import type { Metadata } from 'next'
import Link from 'next/link'
import JsonLd from '@/components/JsonLd'
import EnglishNote from '@/components/landing/EnglishNote'
import Shell from '@/components/landing/Shell'
import { LINKS } from '@/lib/site'
import { breadcrumbs, pageMetadata } from '@/lib/seo'

export const metadata: Metadata = pageMetadata({
  path: '/security',
  title: 'security',
  description:
    'how to report a vulnerability in noah or this site, and how every release is verified: an ed25519-signed manifest and a sha-256 for each installer.',
})

/** The ed25519 public key compiled into noah (crates/auto_update/release_public_key.txt). */
const RELEASE_PUBLIC_KEY = '50e21800e89b09f431a54249efdb39ef770563b87734f56142a326e2c536f5e2'

export default function SecurityPage() {
  return (
    <Shell>
      <JsonLd data={breadcrumbs([{ name: 'security', path: '/security' }])} />
      <main className="l-doc">
        <div className="l-doc-card">
          <p className="l-doc-meta">security · #houseofasher</p>
          <h1>Security</h1>
          <EnglishNote />
          <p className="l-doc-lede">
            How to tell us about a weakness, and how to know the noah you run is the one we published.
          </p>

          <section>
            <h2>Report a vulnerability</h2>
            <p>
              If you find a security problem in noah, shepherd, the updater or this site, report it
              privately through GitHub&apos;s private vulnerability reporting on{' '}
              <a href={`${LINKS.source}/security`} target="_blank" rel="noopener noreferrer">
                shep95/noah
              </a>{' '}
              (Security → Report a vulnerability). If that is not available to you, message{' '}
              <a href={LINKS.twitter} target="_blank" rel="noopener noreferrer">
                @house_ofasher
              </a>{' '}
              or a moderator in the{' '}
              <a href={LINKS.discord} target="_blank" rel="noopener noreferrer">
                discord
              </a>{' '}
              and ask for a private channel. Please do not post details publicly.
            </p>
            <p>A useful report says what is affected, the version, the steps to reproduce it, and what an attacker gains.</p>
            <p>
              Act in good faith: do not access other people&apos;s data, do not degrade the site or its
              downloads for anyone else, and give us reasonable time to ship a fix before you disclose.
            </p>
          </section>

          <section>
            <h2>How releases are verified</h2>
            <p>
              Installers are served only from this site, over HTTPS. Each one has a SHA-256 listed on
              the <Link href="/download">download page</Link> and published beside it as{' '}
              <code>/downloads/&lt;file&gt;.sha256</code>. Check yours with{' '}
              <code>Get-FileHash</code> on Windows or <code>sha256sum</code> on Linux.
            </p>
            <p>
              noah updates itself from <code>/downloads/latest.json</code>, a manifest that names each
              installer with its address and SHA-256. The manifest is signed with noah&apos;s ed25519
              release key, which this site does not hold. noah checks the signature
              against the public key built into the app and refuses the update if it does not match,
              then checks the downloaded installer against its SHA-256 before installing it. Whoever
              controls this website alone cannot ship an update.
            </p>
            <p>
              The signature covers this text, with the asset lines sorted:{' '}
              <code>noah-release\n&lt;build&gt;\n&lt;version&gt;\n&lt;asset&gt; &lt;url&gt; &lt;sha256&gt;\n…</code>
            </p>
            <p>
              Release public key (ed25519, hex): <code>{RELEASE_PUBLIC_KEY}</code>
            </p>
            <p>
              The installers are not code-signed with an operating system certificate yet, so
              Windows SmartScreen does not recognise them. The SHA-256 is how you confirm a download
              by hand until they are.
            </p>
          </section>

          <section>
            <h2>This site</h2>
            <p>
              The site has no accounts, no forms and no third-party scripts. It is served only over
              HTTPS with a strict content security policy, and it cannot be framed by other sites.
              Our <a href="/.well-known/security.txt">security.txt</a> points here.
            </p>
          </section>
        </div>
      </main>
    </Shell>
  )
}
