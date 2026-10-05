# Launcher, actions and host compatibility

[简体中文](../zh-CN/developer-guide/launcher.md) · [Manifest](manifest.md) · [SDK](sdk.md)

Available from Framely **0.4.3-preview.2**. Existing plugins need no changes: a short press opens their quick page and a long press offers Quick panel, Manage plugin and Uninstall plugin. A missing quick page disables that menu item. Uninstall always asks for confirmation.

## Host version requirement

```json
"engines": {"framely": ">=0.4.3-preview.2 <0.5.0"}
```

`engines.framely` is a mandatory host compatibility check, separate from `apiVersion` (the protocol version). It is optional for existing plugins and required when using launcher declarations. A declared launcher feature must have an explicit version floor of `0.4.3-preview.2` or newer.

Ranges use the same parser as plugin dependencies: exact versions (`0.4.3` or `=0.4.3`), comparisons, whitespace/comma intersections, `^`, `~`, and wildcards. `||` and hyphen ranges are not supported. A normal range excludes preview versions; to include a preview, explicitly name its major/minor/patch and prerelease in a comparator. For example `>=0.4.3-preview.2 <0.5.0` matches later 0.4.3 previews and compatible stable releases, but does not automatically include 0.4.4 previews. Build metadata does not affect matching.

The host checks before installation side effects, updates, version rollback, and runtime startup. Incompatible versions remain visible in the library but cannot be installed; dependency resolution skips them. Downgrading Framely preserves incompatible plugins and their enabled preferences while blocking execution. Published older hosts reject the unknown fields rather than run a new-format package; their error messages cannot be retroactively upgraded.

## Entry behavior

```json
"ui": {
  "quickPage": "page.js",
  "windows": {"main": {"entry": "page.js", "title": "Example"}},
  "launch": {
    "launcher": {"type": "window", "window": "main"},
    "quickPanel": {"type": "quickPage"},
    "manager": {"type": "window", "window": "main"}
  }
}
```

Omitted entry behavior opens the plugin's quick page. The manager's separate management button is unaffected. The built-in **Quick panel** menu action always opens the quick page regardless of entry configuration. Window references must name a declared window; a `quickPage` target requires a declared quick page.

`plugin.open` retains its existing backend-start meaning. It does not apply entry navigation.

## Long-press actions

Declare buttons in order; Framely renders them above its three built-in actions. At most 16 custom actions are allowed, with unique valid IDs and nonempty labels up to 120 bytes. Labels are developer-provided text; choose localized content in your published package as appropriate.

```json
"launcherActions": [
  {"id": "open", "label": "Open window", "target": {"type": "window", "window": "main"}},
  {"id": "connect", "label": "Connect", "target": {"type": "backend", "method": "connect", "params": {"profile": "default"}}},
  {"id": "notify", "label": "Send notification", "target": {"type": "frontend", "entry": "actions.js"}}
]
```

A backend target requires a backend and receives `{params, context}`; reserved `framely.*` methods are prohibited. A frontend target names a hashed payload bundle. It runs in a dedicated sandbox only when invoked, without rendering the quick page, and uses the existing scoped SDK capabilities. Frontend actions have a 90-second host deadline; failures stay visible for retry. Actions do not gain plugin management or arbitrary host-command privileges.

Build a separate `actions.ts` bundle (the scaffold's build command does this automatically):

```ts
import {registerLauncherActions, framely} from '@framely/sdk';
registerLauncherActions({
  notify: async context => {
    await framely.notifications.send({
      id: 'hello', title: 'Hello', body: `From ${context.source}`, inbox: true,
    });
  },
});
```

## Startup context

```ts
const initial = await framely.ui.launchContext.get();
const unsubscribe = framely.ui.launchContext.onChanged(context => {
  console.log(context.source, context.trigger, context.actionId);
});
```

`source` is `launcher`, `quickPanel`, or `manager`; `trigger` is `shortPress` or `menuAction`; custom menu actions additionally include `actionId`. Initial context is available after loading, and subsequent activations of a reused page/window send `ui.launch`. Frontend action callbacks receive context directly; backend actions receive it with their parameters. A backend `onStart` lifecycle context also includes `launchContext` when startup was caused by an entry launch. Old calls without a known entry use the quick-panel default.
