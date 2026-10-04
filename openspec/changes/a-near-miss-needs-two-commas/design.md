# Design

## Which artifact is wrong

Both describe a threshold; they differ on its value. The question is which one
to move, and it is settled by where the reasoning is.

The code carries a measurement — eight hand-written tagged lines, six false
offers at one comma — and the spec carries no argument for one comma at all. It
simply says "a comma", in a sentence whose purpose is to enumerate *which* two
misses are recognised rather than to set a threshold. The threshold looks like a
detail of the sentence rather than its subject, which is how it came to be
written loosely and how the drift went unnoticed.

So the spec moves, and it gains the reasoning, because a number with no reason
beside it is a number the next person will change.

## Why the existing scenarios could not catch this

Every scenario under the requirement uses three items:

```
#groceries 7 apples, rye flour, 1kg lentils
```

Two commas. One-comma and two-comma rules agree on it, and on every other
example in the document. The requirement text and the code could disagree
indefinitely without a single scenario failing.

That is the general shape worth noticing: a scenario set built from the happy
example tests the *form* and not the *boundary*. The fix is one scenario on the
other side of the line, which is `#health slept badly, again` — a real sentence
from the measurement, and the one the loose rule got wrong.
