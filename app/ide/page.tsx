'use client'

import { useEffect, useState } from 'react'
import dynamic from 'next/dynamic'
import { loadSettings } from '@/lib/settings'

const IDELayout = dynamic(() => import('@/components/ide/IDELayout'), { ssr: false })

export default function IDEPage() {
  const [settings, setSettings] = useState(() => loadSettings())

  useEffect(() => {
    setSettings(loadSettings())
  }, [])

  return (
    <IDELayout
      wallpaperUrl={settings.wallpaperUrl}
      wallpaperBrightness={settings.wallpaperBrightness}
    />
  )
}
