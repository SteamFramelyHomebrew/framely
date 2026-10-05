# Plugin lifecycle

[简体中文](zh-CN/plugin-lifecycle.md)

Lifecycle is scheduled by the root core, but hooks run as the plugin's declared user, defaulting to `steamos`. Standalone hooks must match backend identity. UI-only plugins can set `lifecycle.runAs` for standalone commands; users see this identity during installation.

```json
{
  "backend": {"entry": "backend.py", "runAs": "steamos", "autostart": false, "restart": "on-failure", "restartLimit": 3},
  "lifecycle": {
    "onInstall": {"entry": "backend.py"},
    "onUpdate": {"entry": "backend.py"},
    "onStart": true, "onStop": true,
    "onUninstall": {"entry": "backend.py"},
    "onCrashCleanup": {"entry": "backend.py", "args": ["--cleanup"]},
    "timeoutSeconds": 10
  }
}
```

All hooks are optional. Command entries must be hashed payload files, never shell strings or outside paths. Hooks do not introduce extra network/identity permissions. Timeouts are per hook, 1–15 seconds, default 10.

| Hook | Timing and execution |
| --- | --- |
| `onInstall` | Standalone command before first activation |
| `onUpdate` | Target version's command after stopping the old backend, before activation; includes downgrades |
| `onStart` | `framely.lifecycle.start` RPC after backend launch, before business calls |
| `onStop` | `framely.lifecycle.stop` before process termination during disabling, restart, updates, uninstall or normal manager shutdown |
| `onUninstall` | Standalone command after stopping backend, before removing payload |
| `onCrashCleanup` | Standalone cleanup after crashes, signals, OOM, timeouts or initialization failure |

Closing UI unmounts React, not the backend. Effects clean page subscriptions/leases, not backend resources. On-demand/autostart determines startup timing; restart policy handles failures after startup.

## Context and SDK

```ts
interface LifecycleContext {
  pluginId: string;
  phase: 'onInstall'|'onUpdate'|'onStart'|'onStop'|'onUninstall'|'onCrashCleanup';
  reason: string;
  version: string;
  previousVersion: string|null;
  dataDir: string;
  exit: {reason:string;exitCode?:number|null;signal?:number|null;oom?:boolean;message?:string}|null;
}
```

Standalone commands read `FRAMELY_LIFECYCLE` and JSON `FRAMELY_LIFECYCLE_CONTEXT`. Backends and commands receive `FRAMELY_PLUGIN_ID`, `FRAMELY_PLUGIN_VERSION`, `FRAMELY_DATA_DIR` and the runtime user's HOME. Standalone stdout/stderr are logs; exit 0 means success. Backend hooks reply with result/error. Pages cannot call reserved `framely.lifecycle.*` methods.

The Python helper is copied from `@framely/sdk/python` into `framely.py`, without extra pip dependencies:

```python
from framely import serve

def initialize(context):
    # Keep operations repeatable and limited to this plugin's resources.
    return {'ready': True}

def cleanup(context):
    return {'cleaned': True}

def dispatch(method, params):
    return {'ok': True}

serve(dispatch, {
    'onInstall': initialize, 'onUpdate': initialize,
    'onStart': initialize, 'onStop': cleanup,
    'onUninstall': cleanup, 'onCrashCleanup': cleanup,
})
```

With a standalone hook environment, the helper calls that hook and exits instead of entering RPC. TypeScript backends can use `registerLifecycle` from `@framely/sdk/lifecycle` in their existing dispatcher; it does not provide process launch or JSON-line transport. Page exports include context types, not backend privileges.

## Failure, cleanup and recovery

Failed install/update commands prevent activation. If installation/update starts a backend immediately (resident or previously running), failed initialization restores the previous version/current link; first installation is undone. First installation of an on-demand plugin normally runs onInstall only. Its onStart runs when a page opens or a business call starts the backend; failure then follows startup recovery, without undoing a previously completed installation. Failed downgrade initialization restores the version used before downgrade. Files can be restored, but migrations and hardware operations cannot be automatically reversed. Make migrations repeatable, use backups/temporary files/atomic replacement, and retain compatibility with the preceding version.

Failed/timed-out `onStop` is logged but the process group is still stopped. Crashes close plugin windows/notifications before standalone cleanup; cleanup failure does not suppress errors or future recovery. Cleanup must not depend on crashed memory or destructors. Remove owned resources only; do not restart SteamVR or delete other plugins' files.

Failed `onUninstall` retains the plugin and allows retry or explicit force removal. Force still attempts bounded cleanup and removes the payload afterward; data remains unless the core's explicit purge option is used. It cannot guarantee restoration of external settings.

Restart defaults to `on-failure`; `never` reports failure without retrying. Limits are 1–10, default 3 consecutive failures, reset after 60 seconds stable operation. Delays are 1, 2, 4, 8, 16, 32 seconds, capped at 32. Exit 0 and manager-directed stops do not count as failures. Exit codes, signals, systemd OOM and timeouts appear in status/logs.

States include starting/running/stopping/stopped/recovering/failed. Core management is serialized. Normal SIGTERM/SIGINT attempts to stop plugins; forced termination or power loss cannot guarantee hooks. Support safe recovery on next startup.

The log view includes backend and lifecycle logs; each retains a current 2 MiB file and a preceding copy. Failed pre-registration installs log to `/var/lib/framely/logs/<id>.lifecycle.log`. User data is under `/var/lib/framely/data/<id>/<identity>/`, relative to any custom core state root.

## Launch context

[Launch context](developer-guide/launcher.md)
