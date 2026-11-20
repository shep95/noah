/** Structured data for search engines. It is a data block, never executed, so the page's script policy does not apply to it. */
export default function JsonLd({ data }: { data: object }) {
  // "<" is escaped so no string in the data can close the script element early.
  const json = JSON.stringify(data).replace(/</g, '\\u003c')
  return <script type="application/ld+json" dangerouslySetInnerHTML={{ __html: json }} />
}
