# asherin.ledger

this folder is asherin.ledger, the person's money room. it holds what noah already knows about where the person's money goes, gathered in one place, so a month can be read at a glance. shepherd keeps the files; the person reads `month.md` and asks questions.

## where the numbers come from

read these; never invent a figure.

- **ai spend**: `spend.jsonl` in noah's data folder (on linux `~/.local/share/noah/spend.jsonl`, on macos `~/Library/Application Support/noah/spend.jsonl`, on windows `%LOCALAPPDATA%\noah\spend.jsonl`). one json object per line: `time`, `provider`, `model`, `input_tokens`, `output_tokens`, `usd`.
- **receipts**: files the person drops into `receipts/` here (pdf, image, email saved as `.eml` or text). read the store, date, total and currency off each one. if a receipt can't be read, list it under "couldn't read" with its file name.
- **subscriptions**: `subscriptions.json` here, a list the person and shepherd keep: `[{"name": "…", "amount": 9.99, "currency": "USD", "every": "month", "next": "2026-11-01", "cancel": "https://…"}]`. add one when a receipt shows a repeating charge, and say so.

## the files you keep

- `month.md`: the current month. three short tables (ai spend by model, receipts by store, subscriptions due), then a total per currency, then one line on what changed against last month. no advice unless asked.
- `months/2026-09.md` and so on: last month's `month.md`, moved there on the first look of a new month.
- `ledger.json`: every line you counted, `{"date", "what", "amount", "currency", "source"}`, so a figure in `month.md` can always be traced to its line.

## rules

- money is the person's private business: nothing in this folder leaves the machine. no web lookups of amounts, no uploading receipts anywhere.
- currencies are never converted silently. if you convert, name the rate and its date.
- when a figure is an estimate (a receipt with a smudged total, a price per token that changed), mark it with `~`.
- the person asks "what did i spend on x": answer from `ledger.json` with the lines that make up the total.
