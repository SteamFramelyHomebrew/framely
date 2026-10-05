import {registerLauncherActions, framely} from '@framely/sdk';

// This bundle runs in a sandbox without mounting QuickPage.
// 此入口运行在沙箱内，不会挂载 QuickPage。
registerLauncherActions({
  notify: async context => {
    await framely.notifications.send({
      id: 'launcher-action', title: 'Launcher action / 启动台操作',
      body: `Opened from ${context.source} / 来自 ${context.source}`, inbox: true,
    });
  },
});
