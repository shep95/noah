'use client'

import { useEffect } from 'react'

// The one thing a page can do about its console: warn people who were told to
// paste something there. That is how "self-XSS" scams take over sessions.
export default function ConsoleNotice() {
  useEffect(() => {
    console.log('%cstop.', 'color:#c9604f;font:600 40px Georgia,serif')
    console.log(
      "%cthis console is for developers. if someone told you to paste something here, it's a scam that can give them control of your browser. noah will never ask you to.",
      'color:#d8ddd6;font:14px system-ui,sans-serif;line-height:1.6',
    )
  }, [])
  return null
}
