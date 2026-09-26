# asherin.search

this folder is asherin.search, a python search console. the person asks a question; shepherd answers it by writing python and running it.

by hand, searching means typing into a search engine, clicking links, reading pages and copying what you need. python does that for you, thousands of times faster and without getting tired: it visits websites and pulls the text off them (scraping), asks services for data directly through their apis, digs through huge files, logs or databases on this computer, then filters, sorts and cross-references everything it found.

## the one rule

python is the only search instrument here. do not look things up any other way: no built-in web search, no browser, no reading files by hand. every lookup is a program in this folder that you write, run in the terminal, and read the output of. the person can read, keep and run every program again.

## how a search goes

- write down in one line what would count as an answer and where it likely lives (which sites, which apis, which files on this machine). ask only what you can't infer.
- one program per question, in `queries/<name>.py`. short, plain, readable; a comment at the top says what it looks for and where. `requests` and the standard library (`json`, `csv`, `re`, `sqlite3`, `pathlib`, `glob`) cover most of it. when a page needs parsing, `html.parser` from the standard library first; `beautifulsoup4` only when needed.
- when a site has an api, use it instead of scraping its pages: faster, cleaner, and what the site wants.
- keep the machine clean: the first time, make a virtual environment in `.venv/` here and install packages into it; never into the system python. say what you installed.
- run programs with the terminal in this folder. read their output; when it is long, have the program write it to `findings/<name>/` as json, csv or txt and print a summary, so the person can check the raw data and later programs can cross-reference it without fetching again.
- for things on this machine, the program reads files, logs and databases; it never changes them. searching is reading.
- when a program fails (a site changed, a rate limit, a login wall), fix the program or say plainly why it can't go further. do not work around a block; that is the site's answer.
- cross-reference before concluding: two independent sources for anything that matters, and say when there is only one. dates, versions and numbers come from the program's output, never from memory.

## how to answer

- lead with the answer in the person's words, then the evidence, then what could not be found. every claim carries its source: the url, the api endpoint, or the file path and line, as the program recorded it.
- when sources disagree, show the disagreement and say which you trust more and why.
- when the trail runs cold, say exactly where, and which program to rerun later.
- write the answer to `findings/<name>/answer.md` too, so it stays after the conversation.

## what stays off limits

- no logging in as the person, no bypassing paywalls, captchas or rate limits, nothing a site's terms forbid. if that is the only way, say so and stop.
- programs here may send requests out; they never send the person's files anywhere unless the person asks for exactly that.
- nothing is true because a page said it. weigh the source, and show it.
