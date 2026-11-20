import type { Metadata } from 'next'
import Link from 'next/link'
import JsonLd from '@/components/JsonLd'
import EnglishNote from '@/components/landing/EnglishNote'
import Shell from '@/components/landing/Shell'
import { LINKS } from '@/lib/site'
import { breadcrumbs, faqPage, pageMetadata } from '@/lib/seo'

export const metadata: Metadata = pageMetadata({
  path: '/faq',
  title: 'questions about noah and shepherd',
  description:
    'is noah free? which ai providers does it work with, can models run locally, where do your code and keys go, and how are updates verified? straight answers.',
})

// Each answer is written once and used both on the page and in the FAQPage
// structured data, so the two can never disagree.
const QUESTIONS: { question: string; answer: string }[] = [
  {
    question: 'Is noah free?',
    answer:
      'Yes. noah costs nothing and has no paid tier, no trial and no paywall. Hosted AI models are billed by their provider under your own key; local models cost nothing.',
  },
  {
    question: 'Do I need an account?',
    answer:
      'No. There is no sign-up, no email and no login. Download it, open it, and start.',
  },
  {
    question: 'What is shepherd?',
    answer:
      "shepherd is noah's built-in AI agent. It reads your project before it writes, works through research, planning and development, and keeps building and testing until the work holds up. It can edit files, run commands you approve, and search the web.",
  },
  {
    question: 'Which AI models can I use?',
    answer:
      'Nearly fifty providers, each under your own key: Western ones such as OpenAI, Anthropic, Google, Mistral and xAI; Chinese ones such as DeepSeek, Alibaba Qwen, Moonshot Kimi, Zhipu GLM and MiniMax; Venice, OpenRouter and any OpenAI-compatible service. Local models run through Ollama, LM Studio or llama.cpp. You choose how hard shepherd reasons on each request.',
  },
  {
    question: 'Where are my API keys kept?',
    answer:
      "In your system's keychain. They never reach our servers; each key goes only to the provider it belongs to, when you use that provider.",
  },
  {
    question: 'Can I run models locally, offline?',
    answer:
      'Yes. Point noah at a model served by Ollama, LM Studio or llama.cpp on your machine. The only limit is your hardware: whatever your memory can hold, from 7B to 405B.',
  },
  {
    question: 'Where does my code go?',
    answer:
      'Your projects stay on your machine. noah keeps no records of them and collects no telemetry. When you use a hosted model, your prompt and the code you include go straight to that provider under your key; noah is not a middleman.',
  },
  {
    question: 'Does shepherd search the web?',
    answer:
      'Yes, without an account or key. Searches go to DuckDuckGo, noah asks you before each one, and the sources are listed under the answer.',
  },
  {
    question: 'What else is in noah besides the editor?',
    answer:
      'asherin.chat, for thinking with shepherd outside any project; asherin.pages, where shepherd makes PDFs, digital books and slideshows; asherin.eye, a live 3D globe intelligence console you can extend; a device room with security checks, system health, a system-wide ad blocker and a duplicate-file cleaner; and a browser room you share with shepherd. Your chats are kept in a history you can pin.',
  },
  {
    question: 'What is asherin.search?',
    answer:
      'A research engine you talk to. You type what you want to know; shepherd turns it into searches, page reads, API calls and small python programs, runs them, and brings back what it found the way a browser would: the answer, the links with previews, and a map of how the sources connect to your question and to each other. Every program it wrote and every finding stays in a folder you can read and rerun.',
  },
  {
    question: 'Can I replace shepherd’s brain?',
    answer:
      'Yes, whole. Pick any text file with "noah: replace shepherd brain", or write or paste one of any length with "noah: write shepherd brain". From the next conversation on, that file is the brain; the built-in one is not mixed in. "noah: use built-in shepherd brain" puts it back. The built-in brain stays inside the program and is never written out.',
  },
  {
    question: 'I have an idea but no project. Where do I start?',
    answer:
      'On the welcome page, press "talk to shepherd". noah makes a folder for you under noah-projects, opens it, and shepherd waits for your words. Say what you want built; it asks at most one question and starts.',
  },
  {
    question: 'Can something I build become a tab in noah?',
    answer:
      'Yes. Any page or app that runs in a browser can sit on the rail as a tab of its own: pin it from the browser room, or run "noah: pin project as tab" in a project that has an index.html or a .noah/app.json naming its address. It is kept on your device only.',
  },
  {
    question: 'What is asherin.board?',
    answer:
      'A whiteboard you and shepherd share, over your wallpaper. Draw with a pen, lines, boxes, circles and text, drop in photos with rounded corners or paste them, and ask shepherd in the conversation to draw; it edits the same board file and the board follows within a second.',
  },
  {
    question: 'Can I record the screen with my camera in the corner?',
    answer:
      'Yes. The rail records the noah window; turn on "your camera in recordings" and the next recording carries your camera as a small rounded rectangle in the lower right. It reads the camera through ffmpeg, so ffmpeg has to be installed.',
  },
  {
    question: 'Can shepherd audit my code for security flaws?',
    answer:
      'Yes. Ask for a security audit and shepherd follows its built-in audit skill: it maps the attack surface, searches for each weakness class with the terminal, proves every finding before reporting it, patches with a test that failed before and passes after, and lists what it checked and what it did not.',
  },
  {
    question: 'Is there a way to make it all go quiet?',
    answer:
      'ctrl-alt-q. Every panel closes and every sound, pop-up and badge stops. shepherd keeps working and holds what it needs to tell you; when you press it again, the panels come back and you hear how many things waited.',
  },
  {
    question: 'How does noah keep shepherd in check?',
    answer:
      'Through a trust layer. Finished work comes with evidence: the checks that actually ran and what was not verified. Every change is recorded in a tamper-evident provenance log. Secrets are redacted before the model sees them, text read from the web and from files is screened for prompt injection, and commands can run in a sandbox. Anything irreversible always asks first.',
  },
  {
    question: 'How is noah related to Zed?',
    answer:
      'noah is built on Zed, the open-source editor, under its GPL-3.0 license. It keeps Zed’s speed and native rendering, removes the account and cloud features, and adds shepherd, bring-your-own-key providers and the wallpaper-driven interface.',
  },
  {
    question: 'Which systems does it run on?',
    answer:
      'Windows 10 and 11, and 64-bit Linux (a .deb for Debian and Ubuntu family systems, and a .tar.xz for every other distribution). macOS follows on the download page when its build is ready.',
  },
  {
    question: 'Windows says it protected my PC. Is the installer safe?',
    answer:
      'The installer is not code-signed yet, so SmartScreen does not recognise it. Choose More info, then Run anyway. You can check the file against the SHA-256 on the download page first.',
  },
  {
    question: 'How do updates work, and how do I know they are genuine?',
    answer:
      "noah installs new releases itself. Each release manifest is signed with noah's ed25519 release key and lists a SHA-256 for every installer; noah refuses an update whose signature or checksum does not match. The security page explains how to check a download by hand.",
  },
  {
    question: 'What is noah shield?',
    answer:
      'A free, open-source browser extension for Chrome, Edge, Brave, Firefox, Safari and every other browser: a fail-closed proxy tunnel, tracker and data-grab blocking, phishing and scam page breakers, cheaper prices and working coupons while you shop, a fingerprint that changes per site, and a watch on anything that records your screen. Its data feeds are signed with the same release key as noah. The shield page lists every feature and what an extension honestly cannot do.',
  },
]

export default function FaqPage() {
  return (
    <Shell>
      <JsonLd data={faqPage(QUESTIONS)} />
      <JsonLd data={breadcrumbs([{ name: 'faq', path: '/faq' }])} />
      <main className="l-doc">
        <div className="l-doc-card">
          <p className="l-doc-meta">questions</p>
          <h1>Straight answers</h1>
          <EnglishNote />
          <p className="l-doc-lede">
            Anything else, ask in the <a href={LINKS.discord}>discord</a>. Choosing an editor? See{' '}
            <Link href="/compare">how noah compares</Link>. Running models on your own machine? Read the{' '}
            <Link href="/local-models">local models guide</Link>. Ready to try it?{' '}
            <Link href="/download">Download noah</Link>.
          </p>
          {QUESTIONS.map(({ question, answer }) => (
            <section key={question} className="l-faq-item">
              <h2>{question}</h2>
              <p>{answer}</p>
            </section>
          ))}
        </div>
      </main>
    </Shell>
  )
}
