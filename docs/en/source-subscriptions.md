# Plugin source subscriptions

[简体中文](../source-subscriptions.md)

In Sources → Source subscriptions, paste an HTTPS JSON URL, preview and confirm:

```json
{
  "schemaVersion": 1,
  "name": "Community sources",
  "sources": [
    {"id":"stable", "name":"Stable", "url":"https://example.org/stable/catalog.json"},
    {"id":"testing", "name":"Testing", "url":"https://example.org/testing/catalog.json"}
  ]
}
```

Source IDs must remain stable and unique within a subscription. Duplicate URLs, nested subscriptions, HTTP or credential-bearing URLs are forbidden. Limits: 100 sources/512 KiB per subscription, 20 subscriptions/200 sources per device. Subscriptions aggregate catalog URLs, not packages or images.

Automatic refresh defaults to daily and can be disabled or triggered manually. The running session checks due entries at startup, uses ETag/Last-Modified, and follows at most five HTTPS redirects. Failures preserve persisted source lists and retry with exponential backoff from one minute up to one day.

New sources join automatically; names update. URL changes keep the old address until confirmed. Refreshing also refreshes catalogs. Catalog caches are session-local; offline catalogs after restart are not guaranteed.

Canonical URLs deduplicate references from subscriptions and manual additions. Removing a subscription/source removes only its reference; sources disappear only without references. Installed plugins, origins and data remain; origins do not silently switch.

Disabled preferences persist across refresh/removal/readdition. Manage subscription URL changes through pending changes. Refreshing changes source metadata and update hints, never automatically installs, updates or enables plugins.
