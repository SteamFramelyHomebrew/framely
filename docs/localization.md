# Languages and community translation

[简体中文](zh-CN/localization.md)

Framely includes Simplified Chinese and English. Choose a language at the top of Settings; automatic follows the system and manual choices persist. Unsupported languages and missing translations fall back to built-in English.

## Create a language file

Download the English template from Settings or copy `ui/locales/en-US.json`. Use UTF-8 JSON:

```json
{
  "schemaVersion": 1,
  "locale": "fr-FR",
  "name": "Français",
  "messages": {"语言": "Langue", "保存": "Enregistrer", "启用 {0}": "Activer {0}"}
}
```

- Use a locale code such as `en-US`, `zh-CN`, `fr-FR`; `auto` is not a file locale.
- Name the language in that language.
- Message keys are stable: translate values only, never keys.
- Preserve all `{0}`, `{1}` placeholders; their positions may change.
- Partial translations fall back to English; empty strings are valid translations.
- Translate whole sentences for natural usage, taking fragment whitespace into account.
- Files contain text only, not executable code or interpreted HTML. Limits: 1 MiB/3000 entries per file and 50 imported files.

## Install

In Settings → Language, install the JSON, then select it. Reimporting the same locale replaces that language file.

Files live in `/var/lib/framely/locales/<locale>.json`. Administrators may place matching filenames there directly and refresh the panel. Invalid files are skipped and named in settings. Plugin names, descriptions, tags and pages remain the plugin author's responsibility; host language files do not translate them.

## Validate the project

Run `node tools/test-localization.mjs` for English coverage, placeholders, system matching, manual preferences and fallback behavior.
