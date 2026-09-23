---
title: Compatibility
---

# Compatibility

The compatibility target is **komga 1.27.1**: the Flyway migrations, the OpenAPI document, and the behavior fixtures are taken from that release, and API behavior is ported from it. Known deviations are listed under [Known limitations](./limitations.md).

## Differential test harness

`tests/diff/diff.py` starts the Java komga and kmrs side by side over the same fixture library and compares ~105 endpoints (status, normalized JSON/XML bodies, headers, zip structure):

```sh
python3 tests/diff/diff.py --java-jar /path/to/komga.jar --rust-bin ./target/debug/kmrs
```
