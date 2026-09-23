import type { Metadata } from 'next'

export const metadata: Metadata = {
  title: 'IDE — unlocket',
  description: 'Shepherd-powered code editor. Free, open, yours.',
  robots: { index: false, follow: false },
}

export default function IDELayout({ children }: { children: React.ReactNode }) {
  return <>{children}</>
}
