import React, {useState} from 'react';
import {registerPlugin, framely, Section, Button, TextField, Notice} from '@framely/sdk';

function QuickPage() {
  const [text, setText] = useState('');
  const [message, setMessage] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  async function settings(save: boolean) {
    setBusy(true); setError(''); setMessage('');
    try {
      const result = await framely.call<{text: string}>(save ? 'settings.set' : 'settings.get', {text});
      setText(result.text); setMessage(save ? '已保存到插件数据目录。' : '已读取。');
    } catch (e) {setError(String(e));} finally {setBusy(false);}
  }
  return <Section title="我的插件">
    <TextField label="保存一段文字" value={text} onChange={setText}/>
    <Button disabled={busy} onClick={() => void settings(false)}>读取</Button>
    <Button disabled={busy} onClick={() => void settings(true)}>保存</Button>
    <Button onClick={() => void framely.windows.open('main')}>打开大窗口</Button>
    {message && <Notice>{message}</Notice>}
    {error && <Notice error>{error}</Notice>}
  </Section>;
}
function WindowPage() {
  const [error, setError] = useState('');
  async function notify() {
    try {await framely.notifications.send({id: 'hello', title: '你好', body: '插件已准备就绪。'});}
    catch (e) {setError(String(e));}
  }
  return <Section title="独立窗口">
    <Button onClick={() => void notify()}>发送通知</Button>
    <Button onClick={() => void framely.windows.close('main')}>关闭窗口</Button>
    {error && <Notice error>{error}</Notice>}
  </Section>;
}
registerPlugin({QuickPage, WindowPage});
