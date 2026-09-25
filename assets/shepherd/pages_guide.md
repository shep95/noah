# asherin.pages

this folder is asherin.pages: the person describes a document and shepherd makes it. every piece is a folder here, named `asherin.<name>` after what the person calls it.

## what to make

- **a pdf document** (report, letter, résumé, guide): one `index.html` with print styles (`@page` size and margins, `break-before`, no web-only chrome). export it with the browser tool: open the file, then `pdf <folder>/<name>.pdf`.
- **a digital book**: one html file per chapter plus `index.html` with the contents. for an ebook, also package an epub (`mimetype` stored first and uncompressed, `META-INF/container.xml`, an OPF with the chapters in order, a nav document) zipped with the terminal. a print version is the same chapters exported to pdf.
- **a slideshow**: one `index.html` with a `<section>` per slide at 16:9, arrow keys and space to move, `f` for full screen, the slide number in a corner. no external frameworks; it has to open offline. export to pdf with one slide per page.

## how to make it land

- start by asking only what you can't infer: who it's for and the one thing they should feel or do after. then draft the whole piece; don't ask section by section.
- open with the strongest line. each page or slide carries one idea; cut anything that doesn't serve it.
- typography first: two typefaces at most, a clear scale, generous line height, 60–75 characters per line for reading text.
- if the person uploads a background or image, put it in `assets/` and design around it: sample its colors for the palette, keep text on a calm area or over a soft scrim so it stays readable (4.5:1 contrast), and never stretch it.
- when they don't give a direction, pick one deliberately (quiet editorial, bold poster, warm book) and say which in one line, so they can redirect.
- show the work: after each change, preview the file in the browser room (the person sees it live) and tell them what changed.
- keep every source file editable by hand; write plain html and css they can read.
