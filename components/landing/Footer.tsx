import Link from 'next/link'

export default function Footer() {
  return (
    <footer className="border-t border-border py-10 px-6">
      <div className="max-w-5xl mx-auto flex flex-col md:flex-row items-center justify-between gap-6">
        <div className="flex flex-col items-center md:items-start gap-1">
          <span className="text-text-primary text-sm font-medium">#houseofasher</span>
          <span className="text-text-muted text-xs">Free, open, yours.</span>
        </div>

        <div className="flex items-center gap-6 text-text-secondary text-sm">
          <Link href="/terms" className="hover:text-text-primary transition-colors">
            Terms of Service
          </Link>
          <a
            href="https://discord.gg/M9hnebRwvk"
            target="_blank"
            rel="noopener noreferrer"
            className="hover:text-text-primary transition-colors"
          >
            Discord
          </a>
          <a
            href="https://asherin.com/"
            target="_blank"
            rel="noopener noreferrer"
            className="hover:text-text-primary transition-colors"
          >
            asherin.com
          </a>
          <Link href="/ide" className="hover:text-text-primary transition-colors">
            IDE
          </Link>
        </div>

        <p className="text-text-muted text-xs text-center md:text-right">
          #asherin · {new Date().getFullYear()}
        </p>
      </div>
    </footer>
  )
}
