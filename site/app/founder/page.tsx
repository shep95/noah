import fs from 'node:fs'
import path from 'node:path'
import type { Metadata } from 'next'
import JsonLd from '@/components/JsonLd'
import EnglishNote from '@/components/landing/EnglishNote'
import Shell from '@/components/landing/Shell'
import { LINKS } from '@/lib/site'
import { FOUNDER_NAME, breadcrumbs, founderProfile, pageMetadata } from '@/lib/seo'

export const metadata: Metadata = pageMetadata({
  path: '/founder',
  title: `${FOUNDER_NAME}, founder`,
  description:
    'asher shepherd newton, prompt engineer and founder of house of asher and asherin, the maker of noah.',
  openGraphType: 'profile',
})

const NAME = FOUNDER_NAME

const SOCIALS = [
  { label: 'asherin.com', href: LINKS.asherin },
  { label: 'instagram', href: LINKS.instagram },
  { label: 'twitter', href: LINKS.twitter },
]

// The photo is optional at build time: until public/founder.jpg exists the
// page shows a monogram instead of a broken image.
function hasPhoto() {
  return fs.existsSync(path.join(process.cwd(), 'public', 'founder.jpg'))
}

export default function Founder() {
  const photo = hasPhoto()
  return (
    <Shell>
      <JsonLd data={founderProfile({ photo: photo ? '/founder.jpg' : null })} />
      <JsonLd data={breadcrumbs([{ name: 'founder', path: '/founder' }])} />
      <main className="l-doc">
        <div className="l-doc-card l-founder">
          <p className="l-doc-meta">founder</p>
          <div className="l-founder-body">
            {photo ? (
              // eslint-disable-next-line @next/next/no-img-element
              <img className="l-founder-photo" src="/founder.jpg" alt={NAME} />
            ) : (
              <div className="l-founder-photo l-founder-monogram" aria-hidden>
                a
              </div>
            )}
            <div className="l-founder-text">
              <h1 className="l-founder-name">{NAME}</h1>
          <EnglishNote />
              <p className="l-founder-role">prompt engineer</p>
              <p className="l-founder-title">
                emperor of #houseofasher, the digital empire, and #asherin, the physical empire.
              </p>
              <ul className="l-founder-links">
                {SOCIALS.map((social) => (
                  <li key={social.label}>
                    <a href={social.href} target="_blank" rel="noopener noreferrer me">
                      {social.label}
                    </a>
                  </li>
                ))}
              </ul>
            </div>
          </div>
        </div>
      </main>
    </Shell>
  )
}
