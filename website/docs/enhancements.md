---
title: Enhancements
---

# Enhancements

Improvements kmrs adds on top of Java parity, plus intentional behavior differences. Accepted gaps that are not planned to be closed are listed under [Known limitations](./limitations.md).

## Capabilities the Java version does not have

- **Search**: simplified ↔ traditional Chinese cross-search and CJK boundary unigrams (e.g. `3月` matches `3月的狮子`). Details: [Search](./search.md).
- **Natural sort with a configurable locale**: numbered titles sort by numeric value ("Page 2" < "Page 10"; a dot is plain text, so "Vol 1.5" < "Vol 1.10", matching Explorer/Finder) under a total order (deterministic pagination boundaries). The sort locale follows `server.sort-locale` / `KOMGA_SORT_LOCALE` (BCP47, e.g. `zh-CN` for pinyin order). Page order and series book numbering use the same comparator. The Java version hard-codes the UCA root and sorts numbered titles lexicographically ("Page 10" < "Page 2").

## Behavior differences

Intentional differences from the Java behavior:

- Page order and series book numbering are case-sensitive; the Java natural comparator is case-insensitive.
