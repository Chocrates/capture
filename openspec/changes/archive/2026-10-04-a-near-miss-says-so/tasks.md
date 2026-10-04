## 1. Recognising the miss

- [x] 1.1 `near_miss(text) -> Option<NearMiss>`, beside `list_of`, sharing its anchoring guard.
- [x] 1.2 `NoColon` and `NothingAfterTheColon`, each carrying the tag so a surface can say where the character goes.
- [x] 1.3 2000 cases over arbitrary text: anything offered as a near miss is still not a list, and is still anchored. The whole safety of this is that the guard does not move.
- [x] 1.4 Quiet on prose, on `#reading finished the long one`, on `#standup at 9:30`, and on anything already a list.

## 2. Saying it

- [x] 2.1 Answered **first**, before anything about time: the grocery capture also reads as "a day" at 70%, and telling somebody their shopping list might be an appointment is worse than useless when the real answer is a missing character.
- [x] 2.2 "This looks like a list. A colon after #groceries would make it one: #groceries: …". **Also found that `capture review` computed suggestions and printed none of them** — a suggestion nobody is shown does not exist. Now printed.

## 3. Accepting it

- [x] 3.1 `mutate::accept_near_miss` writes an ordinary edit. Refused where there is nothing to fix, so a surface offering it on the wrong capture fails loudly rather than rewriting text for no reason.
- [x] 3.2 Against the capture from the real archive: the colon lands after the tag and the five items are the ones the text named.
- [x] 3.3 Reading the offer changes nothing and it is still there afterwards.

## 4. The surfaces

- [x] 4.1 `acceptNearMiss(name)`, answering the text as it now reads. The offer itself arrives through the existing `suggestion(name)`.
- [x] 4.2 The note page offers **"Add the colon"** rather than "Yes, that's what it is", so the gesture that changes the text does not wear the same button as the one that does not.
- [x] 4.3 `capture fix-list <note>` — named for what it does to the text rather than "accept", because this rewrites what somebody wrote and accepting a type does not.
- [x] 4.4 The window offers both, and they do not wear the same button: **"Add the colon"** where the text will be rewritten, **"Yes, that is what it is"** where a type is annotated beside it. `accept_suggestion` and `fix_list` added to the IPC — the desktop had no accept gesture for suggestions at all, so it was showing a sentence nobody could act on.

## 5. Seeing it

- [x] 5.1 Before and after against a scratch store, on the capture from the real archive: silent, then offering the colon, then five tickable items. The measurement in 5.2 is the stronger evidence and is kept as an ignored test.
- [x] 5.2 **Measured, and the rule was wrong.** Over 614 capture-shaped strings from the suite it fired **0 times** — but that corpus holds only ten tag-anchored captures, five already lists, so a zero rate said almost nothing. Against hand-written tagged prose it fired on **every line**: `#reading finished the long one, finally`, `#health slept badly, again`, `#journal a quiet day, mostly reading` — six false offers out of eight.

  The design's open question asked one comma or two. The answer is **two**, which is three things and therefore a list rather than a sentence with a clause in it. Six false positives to zero, both true positives kept. The cost — a two-item list written without its colon stays silent — is pinned as its own test rather than discovered later.
