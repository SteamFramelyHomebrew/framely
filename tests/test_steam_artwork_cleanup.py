"""Execute the Steam cleanup expression against a simulated Steam client."""
import json
import pathlib
import re
import subprocess
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]


class ArtworkCleanup(unittest.TestCase):
    def run_cleanup(self, target):
        source = (ROOT / 'src/apk/steam_ui.rs').read_text().split('pub(super) fn clear_artwork', 1)[1]
        template = re.search(r'r#"(.*?)"#', source, re.S).group(1)
        expression = template.replace('{parameters}', '__PARAMETERS__').replace('{{', '{').replace('}}', '}')
        expression = expression.replace('__PARAMETERS__', json.dumps({'id': 2452903544, 'exe': '/owned/base.apk'}))
        setup = f'''
const calls=[];
const appStore={{m_mapApps:new Map([[2452903544,{{appid:2452903544}}]])}};
const SteamClient={{Apps:{{
  RegisterForAppDetails(id,cb){{cb({{strShortcutExe:{json.dumps(target)}}});return {{unregister(){{}}}};}},
  async ClearCustomArtworkForApp(id,kind){{calls.push(['clear',id,kind]);}},
  SetShortcutIcon(id,path){{calls.push(['icon',id,path]);}}
}}}};
'''
        script = setup + f'''Promise.resolve({expression}).then(()=>console.log(JSON.stringify({{calls,ok:true}})))
            .catch(()=>console.log(JSON.stringify({{calls,ok:false}})));'''
        result = subprocess.run(['node', '-e', script], capture_output=True, text=True, check=True)
        return json.loads(result.stdout)

    def test_clears_every_injected_image_and_shortcut_icon(self):
        result = self.run_cleanup('"/owned/./base.apk"')
        self.assertTrue(result['ok'])
        self.assertEqual(result['calls'], [['clear', 2452903544, kind] for kind in [0, 1, 3, 4]]
                         + [['icon', 2452903544, '']])

    def test_refuses_another_shortcut_with_the_same_id(self):
        result = self.run_cleanup('/other/game.exe')
        self.assertFalse(result['ok'])
        self.assertEqual(result['calls'], [])


if __name__ == '__main__':
    unittest.main()
