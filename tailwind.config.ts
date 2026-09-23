import type { Config } from 'tailwindcss'

const config: Config = {
  content: [
    './pages/**/*.{js,ts,jsx,tsx,mdx}',
    './components/**/*.{js,ts,jsx,tsx,mdx}',
    './app/**/*.{js,ts,jsx,tsx,mdx}'
  ],
  theme: {
    extend: {
      colors: {
        bg: {
          base: '#080c08',
          surface: '#0e130e',
          elevated: '#141a14',
          hover: '#1a231a'
        },
        border: {
          DEFAULT: '#1a2a1a',
          subtle: '#111811'
        },
        text: {
          primary: '#c4d4bc',
          secondary: '#72876c',
          muted: '#445240'
        },
        accent: {
          DEFAULT: '#3a6449',
          hover: '#47785a',
          glow: '#2a4e39'
        },
        fog: {
          light: '#d8e4d4',
          mid: '#a0b49a',
          dark: '#607860'
        }
      },
      fontFamily: {
        sans: ['var(--font-geist-sans)', 'system-ui', 'sans-serif'],
        mono: ['var(--font-geist-mono)', 'Fira Code', 'monospace']
      },
      animation: {
        'fade-in': 'fadeIn 0.6s ease forwards',
        'fade-up': 'fadeUp 0.8s ease forwards',
        'pulse-slow': 'pulse 4s ease-in-out infinite',
        'float': 'float 6s ease-in-out infinite',
        'cursor-blink': 'blink 1.2s step-end infinite'
      },
      keyframes: {
        fadeIn: {
          '0%': { opacity: '0' },
          '100%': { opacity: '1' }
        },
        fadeUp: {
          '0%': { opacity: '0', transform: 'translateY(20px)' },
          '100%': { opacity: '1', transform: 'translateY(0)' }
        },
        float: {
          '0%, 100%': { transform: 'translateY(0)' },
          '50%': { transform: 'translateY(-8px)' }
        },
        blink: {
          '0%, 100%': { opacity: '1' },
          '50%': { opacity: '0' }
        }
      },
      backgroundImage: {
        'gradient-radial': 'radial-gradient(var(--tw-gradient-stops))',
        'gradient-fog': 'linear-gradient(180deg, rgba(8,12,8,0) 0%, rgba(8,12,8,0.7) 60%, rgba(8,12,8,1) 100%)'
      }
    }
  },
  plugins: []
}

export default config
