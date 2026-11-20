import type { Metadata } from 'next'
import Link from 'next/link'
import JsonLd from '@/components/JsonLd'
import EnglishNote from '@/components/landing/EnglishNote'
import Shell from '@/components/landing/Shell'
import { breadcrumbs, howTo, pageMetadata } from '@/lib/seo'

export const metadata: Metadata = pageMetadata({
  path: '/local-models',
  title: 'run ai coding models locally, offline, in noah',
  description:
    'how to run llama, qwen coder and deepseek on your own machine with ollama, lm studio or llama.cpp and use them in noah, the free ai code editor. memory needed per model size, and what to pick.',
})

const STEPS = [
  {
    name: 'Install a local server',
    text: 'Install Ollama (ollama.com), LM Studio (lmstudio.ai) or llama.cpp. Each runs a model on your machine and serves it on a local port; Ollama listens on 11434, LM Studio on 1234.',
  },
  {
    name: 'Pull a coding model',
    text: 'For code, start with qwen2.5-coder (7b or 14b), deepseek-coder-v2, or llama3.3 70b if your machine has the memory. In Ollama: ollama pull qwen2.5-coder:14b.',
  },
  {
    name: 'Point noah at it',
    text: 'Open noah, and in the shepherd settings choose Ollama, LM Studio or an OpenAI-compatible server and enter the local address. noah notices a server already running on this machine and offers it.',
  },
  {
    name: 'Choose the model in the composer',
    text: 'Pick the local model in the model menu under the message box. It stays selected. Everything shepherd sends now stays on your machine.',
  },
]

const SIZES: { model: string; memory: string; fit: string }[] = [
  { model: '7B, 4-bit', memory: 'about 5 GB', fit: 'any recent laptop; quick; fine for edits and questions' },
  { model: '14B, 4-bit', memory: 'about 9 GB', fit: '16 GB machines; the sweet spot for coding' },
  { model: '32B, 4-bit', memory: 'about 20 GB', fit: '32 GB machines or a 24 GB gpu; strong reasoning' },
  { model: '70B, 4-bit', memory: 'about 40 GB', fit: '64 GB machines or two gpus; close to hosted models' },
  { model: '405B, 4-bit', memory: 'about 230 GB', fit: 'workstations and servers; noah does not cap it' },
]

export default function LocalModelsPage() {
  return (
    <Shell>
      <JsonLd
        data={howTo({
          path: '/local-models',
          name: 'Run an ai coding model locally in noah',
          description: metadata.description as string,
          steps: STEPS,
        })}
      />
      <JsonLd data={breadcrumbs([{ name: 'local models', path: '/local-models' }])} />
      <main className="l-doc">
        <div className="l-doc-card">
          <p className="l-doc-meta">guide · local models</p>
          <h1>Run models on your own machine</h1>
          <EnglishNote />
          <p className="l-doc-lede">
            A local model costs nothing per token, works on a plane, and never sees the network. noah treats
            it like any other provider: pick it once and shepherd uses it for everything, reading your project
            and writing code the same way. The only limit is your memory.
          </p>

          <h2>Four steps</h2>
          <ol>
            {STEPS.map((step) => (
              <li key={step.name}>
                <strong>{step.name}.</strong> {step.text}
              </li>
            ))}
          </ol>

          <h2>How much machine a model needs</h2>
          <p>
            Rough figures for 4-bit quantised weights, which is what Ollama and LM Studio serve by default. A
            model needs its weights in memory plus room for the context; gpu memory is fastest, system memory
            works.
          </p>
          <table className="l-stat-table">
            <thead>
              <tr><th scope="col">model size</th><th scope="col">memory</th><th scope="col">where it fits</th></tr>
            </thead>
            <tbody>
              {SIZES.map((row) => (
                <tr key={row.model}><td>{row.model}</td><td>{row.memory}</td><td>{row.fit}</td></tr>
              ))}
            </tbody>
          </table>

          <h2>Which model for code</h2>
          <ul>
            <li><strong>Qwen2.5-Coder</strong> (7B, 14B, 32B): the best all-round local coder at each size; start here.</li>
            <li><strong>DeepSeek-Coder-V2</strong> and <strong>DeepSeek-R1</strong> distills: strong reasoning for harder changes; slower.</li>
            <li><strong>Llama 3.3 70B</strong>: a general model that writes good code when you have the memory.</li>
            <li>Ask shepherd to reason harder on a request when a small model stumbles; noah lets you choose how deep it goes.</li>
          </ul>

          <h2>What stays private</h2>
          <p>
            With a local model nothing leaves your machine: not the prompt, not your files, not the answer. Web
            search is the exception and asks you first, every time. noah itself sends no telemetry. The{' '}
            <Link href="/security">security page</Link> has the rest.
          </p>

          <h2>Mixing local and hosted</h2>
          <p>
            Many people keep a local model for everyday edits and a hosted one, under their own key, for the
            hard hours. Switch in the composer; the conversation carries on. See{' '}
            <Link href="/compare">how noah compares</Link> if you are choosing an editor, or{' '}
            <Link href="/download">download it</Link> and try both.
          </p>
        </div>
      </main>
    </Shell>
  )
}
