# asherin.board

this folder is asherin.board, a whiteboard the person and shepherd share. the person draws on it in noah with a pen, lines, boxes, circles, text and photos; shepherd draws on it by editing `board.json`. the board shows the file as it is, and reloads within a second of a change, so what you write appears while you work.

## the file

`board.json` holds one object: `{"revision": 12, "items": [ … ]}`. `revision` goes up by one with every save, from either side. each item is one of:

```json
{"kind": "stroke", "points": [[x, y], [x, y], …], "color": "ink", "width": 2}
{"kind": "line", "from": [x, y], "to": [x, y], "color": "muted", "width": 2}
{"kind": "rect", "origin": [x, y], "size": [w, h], "color": "accent", "width": 2, "fill": false}
{"kind": "ellipse", "origin": [x, y], "size": [w, h], "color": "blue", "width": 2, "fill": false}
{"kind": "text", "origin": [x, y], "text": "one line", "color": "ink", "size": 16}
{"kind": "image", "origin": [x, y], "size": [w, h], "path": "images/photo.png", "radius": 12}
```

- coordinates are pixels from the board's top-left; the board is as large as the person's window, usually about 1200 by 800, and scrolls if you go past.
- colors are names from the person's own palette (`ink`, `muted`, `accent`, `red`, `green`, `blue`, `yellow`, `white`) or a hex color like `#7aa2f7`. names follow the wallpaper, so prefer them.
- image paths are relative to this folder; put files in `images/`. corners are rounded by `radius`.

## how to draw

- read `board.json` first, every time: the person may have drawn since you last looked. add your items to the end of `items`, raise `revision` by one, and write the whole file back. never drop what is there unless asked to clear or erase.
- draw the way a person would at a whiteboard: boxes with a text label inside, arrows as a line with a short two-segment stroke for the head, a few words rather than paragraphs. leave room; a diagram that breathes reads better than one that fills the board.
- for a diagram of many parts, lay the parts out first (a grid or a row with even gaps), then connect them, then label. keep text at size 14 to 18 and lines at width 2.
- when the person asks you to draw something they described, say in one line what you drew and where; when they ask what is on the board, describe the items in plain words, not json.
- photos the person adds are files in `images/`; you can look at them if a tool lets you read images, and refer to them by position on the board.

## what stays off limits

- this folder is the board and nothing else: no programs, no notes, no files beyond `board.json` and `images/`.
- the person's drawings are theirs: move or restyle them only when asked.
