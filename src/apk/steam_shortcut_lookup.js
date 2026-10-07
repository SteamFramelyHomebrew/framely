async function(p) {
    const executable = d => d.strShortcutExe?.replace(/^"|"$/g, '').replace('/./', '/');
    const details = id => new Promise((resolve, reject) => {
        let sub, settled = false;
        const timer = setTimeout(() => { sub?.unregister(); reject(Error('Details timed out')); }, 3000);
        sub = SteamClient.Apps.RegisterForAppDetails(id, d => { settled = true; clearTimeout(timer); sub?.unregister(); resolve(d); });
        // Steam may invoke a cached callback synchronously.
        if (settled) sub?.unregister();
    });
    let apps, app;
    for (let n = 0; n < 10; n++) {
        apps = Array.from(appStore.m_mapApps.values());
        const matches = apps.filter(a => a.display_name === 'Devkit Game: ' + p.game);
        app = matches.length === 1 ? matches[0] : apps.find(a => a.appid === p.previous);
        if (app) break;
        if (n < 9) await new Promise(r => setTimeout(r, 100));
    }
    if (app) {
        if (app.appid < 2147483648) throw Error('Missing owned shortcut');
        const d = await details(app.appid);
        if (executable(d) !== p.exe) throw Error('Shortcut target does not match owned APK');
        return { id: app.appid };
    }
    // A previous attempt may have renamed the entry before persisting its ID.
    // Recover by exact executable ownership, never by a user-visible app name.
    const shortcuts = apps.filter(a => a.appid >= 2147483648);
    if (shortcuts.length > 256) throw Error('Too many shortcuts to recover owned entry');
    const rows = await Promise.allSettled(shortcuts.map(async a => ({ id: a.appid, details: await details(a.appid) })));
    const owned = rows.filter(r => r.status === 'fulfilled' && executable(r.value.details) === p.exe);
    if (owned.length !== 1) throw Error('Missing or ambiguous owned shortcut');
    return { id: owned[0].value.id };
}
