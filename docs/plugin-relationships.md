# Dependencies and conflicts

[简体中文](zh-CN/plugin-relationships.md)

```json
{
  "dependencies": {
    "example.shared-api": "^1.0.0",
    "example.camera-api": {"version": ">=1.2.0 <2.0.0", "source": "https://example.org/plugins/catalog.json"}
  },
  "optionalDependencies": {"example.notifications": "^1.0.0"},
  "conflicts": {"example.other-camera": "*"},
  "exclusiveResources": ["steamvr.passthrough-color"]
}
```

All four fields are optional. Relationship-declaring plugins and matched dependency/conflict targets use valid SemVer. Ranges support exact versions, `^`, `~`, `*` and comma/space comparison conjunctions. `1.2.0` is exact; `>=1.2.0 <2.0.0` requires both. `||` is unsupported. Self-dependencies, duplicate required/optional entries, contradictory conflicts and repeated exclusive resources are rejected.

`source` is an HTTPS catalog URL, not a subscription or package. Compatible installed versions/sources are reused first. Without explicit source, the originating catalog is preferred; otherwise choose explicitly. An unavailable explicit source is not silently replaced. New explicit sources join the confirmation plan; disabled sources need enabling first. Removing a source retains installed origin URLs for matching.

Local packages with no default source require selecting missing dependency sources:

```bash
framely install plugin.framely --source my-source --approve
framely install plugin.framely --dependency-source example.shared-api=https://example.org/catalog.json --approve
```

Dependency-source flags may repeat, cannot override explicit Manifest sources, and use the UI's resolver. Add `--approve-run-as` only after reviewing an identity change.

The main catalog contains one recommended entry per ID, with history in relative `plugins/<ID>/versions.json`. Users may select old versions or reinstall. ID/version/hash/relations are rechecked. Resolution reuses installed matches, otherwise the first matching source entry. Incompatible constraints, cycles or missing required dependencies stop installation. Missing optional dependencies neither prevent installation nor install automatically.

Plans disclose packages, versions, sources, users, added sources, dependency enabling and conflict disabling. Confirmed operations use the same verified bytes; changed device/source state requires another inspection. Plans allow at most 32 plugins without a total package/expanded-size cap. Download/verify first, then install sequentially and check reverse dependencies.

Persistent transaction logs restore links, origins/enabled state, undo newly added packages and attempt to restart previous backends on failure. Startup recovers interrupted transactions. Data remains; file rollback cannot undo migrations or hardware effects, so make them repeatable and recoverable.

Enabling confirms required dependencies and conflicting plugins to disable. Either side's conflict declaration prevents simultaneous enabling, not installation. Matching exclusive resources work likewise. Disabling/removing a base plugin requires confirming dependent disabling; dependents are not automatically removed.

Backends start in dependency order after base `onStart` succeeds; absent hooks use process startup. Stops reverse that order. Crashed bases pause running dependents in a waiting state without counting dependent failures, then resume them after recovery. Disabled/non-restarting bases need user action.

```tsx
import {framely, useDependencies} from '@framely/sdk';
const states = await framely.dependencies();
// In React: const {items, error} = useDependencies();
```

Only declared required/optional dependencies are returned, with version/enabling/range/availability/runtime state. Changes emit `dependencies.changed`. No cross-plugin backend calls or extra privileges are granted. Conflicts/resources depend on author declarations, not arbitrary file/device detection.

There is no manual plugin rollback API; old-version installation uses the normal plan and `onUpdate`. Transaction recovery is failure handling, not a version picker. Keep same-ID/version content immutable.
