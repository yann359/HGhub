// Offline first-party fixture. Test adapters provide synthetic session grants.
export const meta = {id:"fixture.web-login", name:"网页登录夹具", version:"1.0.0", protocol:1, timeouts:{login:5}};
const site = "https://accounts.example.invalid";
export default function(hg) {
  return {
    media: {getDetail({id}) { return {id, title:"fixture", playlists:[]}; }, resolvePlayback() { return {streams:[]}; }},
    auth: {
      getMethods() { return {methods:hg.env.features.includes("webview") && hg.webview ? [{id:"open",label:"网页登录"}] : []}; },
      async submit({methodId}, ctx) {
        const deadline = ctx.deadline;
        const opening = hg.webview.open({url:site + "/login", doneWhen:{urlPrefix:site + "/account", cookieNames:["session"]}, timeoutSec:methodId === "timeout" ? 1 : 15});
        let paused = false;
        const timer = setTimeout(() => { paused = ctx.deadline === null; }, 50);
        try {
          const result = await opening;
          const visible = await hg.cookies.get({url:result.finalUrl});
          return {status:"ok", user:{id:"fixture", name:JSON.stringify({paused, resumed:ctx.deadline > deadline, keys:Object.keys(result), cookies:visible.length})}};
        } finally { clearTimeout(timer); }
      },
      getProfile() { return null; }, async logout() { await hg.cookies.clear({url:site}); },
    },
  };
}
