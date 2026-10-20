# Homebrew custom-tap draft

This directory is the minimum formula layout to copy into a future custom-tap repository:

```text
homebrew-shellsort/
└── Formula/
    └── shellsort.rb
```

The checked-in formula is deliberately a template. Its project homepage is the configured repository, `https://github.com/a1nbuselvan/Shellsort`; versioned archive URLs and SHA-256 values remain placeholders until matching release assets are published. The custom-tap repository has not been established. Do not publish this formula or describe `brew install` as working while placeholders remain.

Each macOS archive must contain the matching `shellsort` CLI executable and executable `organizer.sh`. Verify archive contents, architecture, `shellsort --version`, the read-only preview, the wrapper confirmation prompt, and the formula test on disposable folders before publishing the tap update.
