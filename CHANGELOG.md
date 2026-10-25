# Changelog

## [0.2.0](https://github.com/4thel00z/codeedit/compare/v0.1.0...v0.2.0) (2026-10-25)


### Features

* outlines for JSON, YAML, Markdown, HTML and XML; inline Markdown ([fb18322](https://github.com/4thel00z/codeedit/commit/fb18322f9d7938f625635f334465be551a3372d3))
* patterns match inside Rust macros and stand in for fields ([108a837](https://github.com/4thel00z/codeedit/commit/108a837d53df02e0f7be34ad133f2d61273ab2e0))
* re-indent query templates, delete one name of a Go declaration ([aaf346a](https://github.com/4thel00z/codeedit/commit/aaf346a3b6fd562837a24cea04ba700c7e052d9a))
* re-indent rewrite templates, lenient punctuation, round-trip get ([5047a9d](https://github.com/4thel00z/codeedit/commit/5047a9d371368e5ff28af45f5637093886769205))
* rename binary to codeedit and add codeedit skill ([d9f7a08](https://github.com/4thel00z/codeedit/commit/d9f7a08b5756c4b4fb6674d409c5fe3431c0af9f))
* treesitter CLI and Python library for structural code edits ([92a2383](https://github.com/4thel00z/codeedit/commit/92a238305329e3529905ed7d8433c1110bea95ce))


### Bug Fixes

* clean ./ diff labels and reject --rewrite on find ([bc4d2ad](https://github.com/4thel00z/codeedit/commit/bc4d2adbcb41a78b8a37c1f252d547894434c0dd))
* JSON dump keeps text of token-only error nodes; faster reindent ([fd9bc59](https://github.com/4thel00z/codeedit/commit/fd9bc59f47cba05d09997144632272f70f0d937f))
* language handling in Python and CLI edit commands; skill update ([6a8a09e](https://github.com/4thel00z/codeedit/commit/6a8a09e5497bb4113b1a64c2d34702256e5db2ce))
* pattern context, selector, constraints and capture expansion ([b9f9008](https://github.com/4thel00z/codeedit/commit/b9f9008a7d98d300174575a68c1868a3467d600c))
* skip only commas and semicolons when matching patterns ([d4b5994](https://github.com/4thel00z/codeedit/commit/d4b599455d442f46aeeb18ac436e94b3a629accf))
* strict JSON input, all-or-nothing writes, error and line-ending checks ([676cdc9](https://github.com/4thel00z/codeedit/commit/676cdc963bbde5d55828fb2390d860f22a66ad98))
* symbol edits on shared lines, brace-free bodies, strings and imports ([c3decd9](https://github.com/4thel00z/codeedit/commit/c3decd94b74940b9eced8277fe3d7deed3e46837))
* symbol extents for comments, decorators and multi-name declarations ([71779cc](https://github.com/4thel00z/codeedit/commit/71779cc4c2c8bc2f7644cf8bc6f2e67c3522ff43))
* trailing comments own the symbol's line; widen overlapped errors ([25b02d4](https://github.com/4thel00z/codeedit/commit/25b02d4c480dcd229b4789ee807beb17a3d81507))


### Documentation

* README, logo, CI and release pipeline ([14d7131](https://github.com/4thel00z/codeedit/commit/14d71314121c15b47dcc39627f82453ec2a8dbbd))
