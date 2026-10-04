# SDK API reference

[简体中文](../zh-CN/developer-guide/sdk.md)

[Development](README.md) · [Manifest](manifest.md) · [Lifecycle](../plugin-lifecycle.md)

Sources are `sdk/src/index.tsx`, `sdk/src/lifecycle.ts` and `sdk/python/framely.py`. Import page APIs from `@framely/sdk`; build copies the Python helper into `payload/framely.py`.

## Register pages

```tsx
import {registerPlugin, Section} from '@framely/sdk';
function QuickPage() {return <Section title="My plugin">Quick content</Section>;}
function MainWindow() {return <Section title="Main window">Details</Section>;}
registerPlugin({QuickPage, windows: {main: MainWindow}});
```

`QuickPage` is required. `WindowPage` is the default independent-window component; `windows` maps declared keys to different components. Missing keys fall back to `WindowPage`, then `QuickPage`. Avoid window key `quick`, which is used by the quick-page route. Pages run in host sandbox iframes, with the bridge installed before your bundle. Registration mounts React, base styles and scrollbars.

## Host API: `framely`

| API | Parameters/result | Purpose |
| --- | --- | --- |
| `framely.call<T>(method, params = {})` | `Promise<T>` | Call your backend; failures reject |
| `framely.windows.open(key)` | Declared key; asynchronous result | Open a declared window |
| `framely.windows.close(key)` | Key; asynchronous result | Close a window without disabling the backend |
| `framely.notifications.send(notification)` | Notification; asynchronous result | Send or replace by ID |
| `framely.notifications.remove(id)` | Notification ID; asynchronous result | Withdraw notification |
| `framely.dependencies()` | `Promise<DependencyStatus[]>` | Query declared dependency state |
| `framely.language.get()` | `Promise<{preference, language}>` | Language preference and effective language |
| `framely.onEvent(callback)` | Unsubscribe function | Receive forwarded events |

These use the plugin bridge, not administrator APIs or other plugins' backends. Handle failures and prevent duplicate submissions while waiting.

```tsx
try {
  const result = await framely.call<{text: string}>('settings.get');
  console.log(result.text);
} catch (error) {console.error(error);}
```

## Notifications

```tsx
await framely.notifications.send({
  id: 'task', title: 'Done', body: 'File processed', durationMs: 8000,
  actions: [{id: 'open', label: 'Open', icon: '↗'}],
});
```

`id`, `title`, `body` are required. Optional `image` accepts PNG/JPEG data URLs or HTTPS, up to a 1 MiB string. At most three actions, each with `id`, `label`, `icon`. `durationMs` is 1000–60000; omission defaults to 8000 ms. Notification/action IDs follow core ID rules; action IDs must be unique within a notification. Title is at most 160 bytes, body 4096, nonempty action label 80, and action icon 16. Each plugin may send at most ten notifications per ten seconds; the global queue permits 64 concurrent notifications. Updating an existing ID does not occupy another slot.

Buttons invoke backend `notification.action` with `{id, action}` and forward a page event of the same name. UI-only plugins can listen for the event. Reuse an ID to update; remove it to withdraw.

## React hooks

| Hook | Result/behavior |
| --- | --- |
| `useBackend<T>(method, params = {})` | `{data, error, loading, call}`; requests are explicit through `call()`, which still throws on failure |
| `usePluginEvent(callback)` | Automatically subscribes and unsubscribes on unmount |
| `useDependencies()` | `{items, error}`; loads automatically and refreshes on `dependencies.changed` |

Return the unsubscribe function from an effect:

```tsx
React.useEffect(() => framely.onEvent(event => console.log(event)), []);
```

`DependencyStatus` includes `id`, `required`, `constraint`, `version`, `enabled`, `matches`, `available`, optional `state.phase`. Constraint is a range string or `{version, source}`; uninstalled versions are null. See [relationships](../plugin-relationships.md).


Language changes emit `{type: "language.changed", data: {preference, language}}`; dependency changes emit `{type: "dependencies.changed"}`. `onEvent` is typed as unknown, so narrow the type before reading fields. Python `emit("progress", {...})` becomes `{type: "progress", data: {...}}`.

## UI components

| Component | Main props |
| --- | --- |
| `Section` | `title`, `children` |
| `Button` | Standard React button props, including `onClick`, `disabled` |
| `Toggle` | `label`, `checked`, `onChange(boolean)`; optional description/disabled |
| `Slider` | `label`, `value`, `onChange(number)`; defaults min 0, max 100, step 1 |
| `TextField` | `label`, `value`, `onChange(string)`; multiline/password/placeholder/disabled |
| `Select` | `label`, `{value,label}[]` options, value/onChange/disabled |
| `Tabs` | `{id,label}[]` tabs, value/onChange |
| `Notice` | children; error styling with `error=true` |

Select is single-valued by default. With `multiple`, value and callback use string arrays; `emptyLabel` and `clearLabel` customize labels. Menus render inside the page for off-screen CEF.

Native input controls automatically use the SteamVR keyboard on Frame, and standard interactive controls get hover feedback. Custom controls need suitable ARIA roles and may use `data-framely-interactive`. Pages do not independently control other views, controllers or system accounts.

## Python backend helper

```python
from framely import serve, emit

def dispatch(method, params):
    if method == 'ping':
        emit('progress', {'percent': 100})
        return {'ok': True}
    raise ValueError('Unknown method')

serve(dispatch)
```

`serve(dispatch, lifecycle=None)` reads newline-delimited requests and translates returns/exceptions into responses. `emit(event, data)` emits events forwarded as `{type, data}`; `notification` can send notifications. Log to stderr; stdout is protocol-only. See [lifecycle callbacks](../plugin-lifecycle.md).

## TypeScript lifecycle helpers

Import `registerLifecycle`, `LifecycleContext`, `LifecyclePhase`, `LifecycleCallbacks` from `@framely/sdk/lifecycle`. Registration returns an asynchronous `(method, context)` dispatcher for reserved lifecycle methods. It does not provide stdin/stdout transport, launch a backend or grant page privileges.

## Preview and privilege boundaries

Development preview simulates windows, notifications and some bridge operations. Test backend calls, real dependencies, VR inputs, haptics and runtime identity on the device. Pages cannot directly call management APIs or reserved `framely.lifecycle.*` methods. Runtime identity is configured in Manifest, not granted by the SDK.

## Headset visibility and capture pause

`framely.ui.getVisibility()` returns `{ known, captureObscured, pageVisible, sequence, sessionId }`. `framely.ui.onVisibilityChanged(callback, onError?)` subscribes and immediately queries the current snapshot; it returns an unsubscribe function. `useVisibility()` provides the same fields and `error`. Capture visibility combines the actual visibility of the quick panel, manager and every plugin window, excluding notifications and dock icons. Remote browser pages have `pageVisible: false` and cannot report headset visibility.

Declare `backend.uiVisibilityEvents: true` to receive the reserved JSON-line RPC `framely.ui.visibility` at startup and on changes, independently of mounted pages. Reply with the request id. The backend snapshot includes `known`, `captureObscured`, `sequence` and `views`; `pageVisible` is frontend-only. A native heartbeat missing for more than two seconds makes `known` false. Capture backends should pause on `!known || captureObscured`. Existing backends do not receive this RPC without opting in. Node/TS can use `registerVisibility` from `@framely/sdk/visibility`; Python supports `serve(dispatch, visibility=callback)`.
