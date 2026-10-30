#!/usr/bin/env python3
"""Production UI checks with synthetic sessions, providers, and diffs.
Requires Python Playwright and a Chromium browser. Build web/dist first.
Optional axe-core path enables a WCAG scan. No actual daemon is contacted.
"""
import argparse, asyncio, copy, json, pathlib, shutil, threading
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from functools import partial
from urllib.parse import urlparse, parse_qs
from playwright.async_api import async_playwright

ROOT = pathlib.Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description="Exercise the production UI with illustrative API fixtures; never starts a real agent.")
parser.add_argument('--output', type=pathlib.Path, default=pathlib.Path('/tmp/plantool-ui-smoke'))
parser.add_argument('--axe-script', type=pathlib.Path)
parser.add_argument('--browser', default=shutil.which('chromium'))
ARGS = parser.parse_args()
OUT = ARGS.output
OUT.mkdir(parents=True, exist_ok=True)
PLAN = '''# Fix session timeout

Keep signed-in customers in their session while they are actively working.

## Approach

Refresh the session before it expires, using the existing authentication endpoint.
Keep a single refresh request in flight and preserve the current page on failure.

## Phase 1: Refresh lifecycle

- [x] Trace the current session expiry behavior
- [ ] Refresh credentials before the expiry window
- [ ] Cancel scheduled refresh on sign-out

## Phase 2: Verify recovery

- [ ] Test expired credentials and intermittent network failures
- [ ] Verify that returning to a background tab refreshes the session

## Affected files

| File | Change |
| --- | --- |
| src/auth/session.ts | Refresh timing and cleanup |
| src/auth/session.test.ts | Expiry and recovery coverage |

## Verification

Run authentication tests and check sign-in, sign-out, and background-tab recovery.
'''

def fixture(stage='plan-review'):
    now = '2026-10-29T10:00:00Z'
    docs = [dict(kind=k, path=f'/home/customer/.plantool/sessions/shop/fix-timeout/{k}.md', exists=True, sha='approved123', lines=30, title='Fix session timeout', captured_at=now, progress=[dict(name='Refresh lifecycle',done=1,total=3),dict(name='Verify recovery',done=0,total=2)]) for k in ['research','plan']]
    return dict(key='shop/fix-timeout',url_path='/s/shop/fix-timeout',dir='/sessions/shop/fix-timeout',session=dict(repo_slug='shop',slug='fix-timeout',title='Fix session timeout',repo=dict(root='/home/customer/projects/shop',common_dir='/home/customer/projects/shop/.git',branch='main'),worktree='/home/customer/projects/shop/.worktrees/fix-timeout',base='main',difftool='difftool',mirror=False,brief='Customers are signed out while working. Keep active sessions alive and handle expired credentials gracefully.',created_at=now),state=dict(stage=stage,seq=5,comments=[],docs={},review=None,updated_at=now,activity=[],milestones=[],proposals=[]),docs=docs,runs=[],open_comments=0,workspace_branch='plantool/fix-timeout',step='Plan',handoff=dict(step='Plan',label='Owner approving',action='Review plan'))

async def check_accessibility(page, label):
    if not ARGS.axe_script:
        return
    await page.add_script_tag(path=str(ARGS.axe_script))
    violations = await page.evaluate('async () => { const r=await axe.run(document,{runOnly:{type:"tag",values:["wcag2a","wcag2aa","wcag21a","wcag21aa"]}}); return r.violations.map(v=>({id:v.id,impact:v.impact,nodes:v.nodes.map(n=>({target:n.target,summary:n.failureSummary}))})); }')
    print({'surface': label, 'accessibility': violations}, flush=True)
    assert not [issue for issue in violations if issue['impact'] in ['serious', 'critical']], violations

async def capture(browser, port):
    context = await browser.new_context(viewport=dict(width=1440,height=1040), device_scale_factor=1, color_scheme='light')
    await context.add_init_script('''window.WebSocket = class { constructor() { window.__mockWS = this; setTimeout(() => this.onopen?.(), 0); } close() {} };''')
    state = fixture()
    empty_inbox = False
    errors=[]
    page=await context.new_page()
    page.on('pageerror', lambda e: errors.append(str(e)))
    async def route(req):
        u = urlparse(req.request.url); path=u.path
        response={}
        if path == '/api/sessions':
            response=[] if empty_inbox else [state]
            for slug,title,stage in ([] if empty_inbox else [('checkout-keyboard','Improve checkout keyboard navigation','research-review'),('receipt-email','Add order receipt emails','implementing'),('search-empty','Refine search empty states','done')]):
                item=copy.deepcopy(state); item['key']=f'shop/{slug}'; item['url_path']=f'/s/shop/{slug}'; item['session']['slug']=slug; item['session']['title']=title; item['state']['stage']=stage; item['session']['worktree']=f'/home/customer/projects/shop/.worktrees/{slug}'; item['workspace_branch']=f'plantool/{slug}'; response.append(item)
        elif path == '/api/providers': response={'providers':[{'id':'claude','available':False,'models':[]},{'id':'codex','available':True,'version':'1.0','models':[]}]}
        elif path == '/api/repos': response={'repos':[{'root':state['session']['repo']['root'],'repo_slug':'shop','branch':'main','base':'main','sessions':1,'last_used':'2026-10-29T10:00:00Z'}]}
        elif path.endswith('/comments'): response={'seq':5,'comments':[]}
        elif path.endswith('/revisions'): response={'approved':None,'viewed':None,'change':None,'pending':False}
        elif '/runs/sample/events' in path: response={'events':[{'seq':1,'at':'2026-10-30T07:00:00Z','event':{'type':'message','id':'sample-result','role':'assistant','content':'The refresh lifecycle is ready for review. I added cleanup on sign-out and coverage for background-tab recovery. Please review the diff before accepting the implementation.'}}]}
        elif '/prompt' in path: response={'prompt':'Read the session brief and write the implementation plan.','stage':'plan','session_stage':state['state']['stage']}
        elif '/changes/file' in path: response={'path':'src/auth/session.ts','diff':'diff --git a/src/auth/session.ts b/src/auth/session.ts\n@@ -1,3 +1,4 @@\n export function refreshSession() {\n-  return expireSession();\n+  scheduleRefresh();\n+  return keepSessionAlive();\n }\n','truncated':False}
        elif '/changes' in path:
            scope=parse_qs(u.query).get('scope',['step'])[0]
            response={'tool':'difftool','scope':scope,'base':('3e91b5c2d7a48f09e6b13c7840a9d2f1b6c5e8a0' if scope=='step' else '19a7ce032b456df0a1c4e9b7d608f235ac17e9b2'),'label':('since this milestone started' if scope=='step' else 'in the whole implementation'),'step_available':True,'stat':[{'path':'src/auth/session.ts','added':18,'deleted':4},{'path':'src/auth/session.test.ts','added':32,'deleted':0}],'review':None}
        elif '/docs/' in path:
            k='research' if '/research' in path else 'plan'
            response={'kind':k,'content':PLAN,'sha':'approved123','headings':[],'progress':state['docs'][0]['progress'],'captured_at':'2026-10-29T10:00:00Z','lines':30}
        elif path.endswith('/stage'):
            state['state']['stage']=req.request.post_data_json['to']; response={'stage':state['state']['stage']}
        elif path.startswith('/api/sessions/shop/fix-timeout'): response=state
        else: response={}
        await req.fulfill(status=200,content_type='application/json',body=json.dumps(response))
    await context.route('**/api/**',route)
    url=f'http://127.0.0.1:{port}'
    await page.goto(url+'/s/shop/fix-timeout?tab=plan',wait_until='networkidle')
    await page.get_by_role('button',name='Approve plan',exact=True).count()
    await page.screenshot(path=str(OUT/'after-plan.png'), full_page=True)
    assert await page.locator('details.actions').get_attribute('open') is not None
    assert await page.get_by_role('button', name='Approve plan',exact=True).is_visible()
    assert not await page.get_by_role('combobox',name='set stage').is_visible()
    await check_accessibility(page,'plan')
    # Default provider is unavailable: opening a run must select Codex.
    await page.get_by_role('button',name='Revise with agent',exact=True).click()
    await page.wait_for_function("document.querySelector('[role=dialog] select')?.value === 'codex'")
    dialog=page.get_by_role('dialog')
    first=dialog.locator('select').first
    await first.focus(); await page.keyboard.press('Shift+Tab')
    assert await page.get_by_role('button',name='Start plan',exact=True).evaluate('(e) => e === document.activeElement')
    await page.keyboard.press('Tab')
    assert await first.evaluate('(e) => e === document.activeElement')
    await page.screenshot(path=str(OUT/'after-run-dialog.png'),full_page=True)
    await check_accessibility(page,'run-dialog')
    await page.get_by_role('button',name='Cancel',exact=True).click()
    assert await page.get_by_role('button',name='Revise with agent',exact=True).evaluate('(e) => e === document.activeElement')
    async with page.expect_response(lambda response: response.url.endswith('/stage')):
        await page.get_by_role('button',name='Approve plan',exact=True).click()
    assert state['state']['stage']=='approved'
    await page.evaluate('window.__mockWS.onmessage({data:JSON.stringify({type:"stage-changed",from:"plan-review",to:"approved"})})')
    await page.get_by_role('button',name='Start implementation',exact=True).click()
    await page.get_by_label('Implementation pace').select_option('step-by-step')
    await page.get_by_role('button',name='Cancel',exact=True).click()
    state['state']['stage']='implementation-review'
    state['step']='Review'
    state['handoff']={'step':'Review','label':'Owner reviewing','action':'Review comments'}
    await page.reload(wait_until='networkidle')
    await page.get_by_role('button',name='Changes',exact=True).click()
    await page.get_by_role('button',name='Expand src/auth/session.ts').click()
    await page.get_by_text('+  scheduleRefresh();').wait_for()
    await page.screenshot(path=str(OUT/'after-changes.png'),full_page=True)
    await page.get_by_role('tab',name='Whole implementation',exact=True).click()
    await page.get_by_text('19a7ce032b456df0a1c4e9b7d608f235ac17e9b2').wait_for()
    await page.get_by_role('button',name='Expand src/auth/session.ts').click()
    await page.get_by_text('+  scheduleRefresh();').wait_for()
    for width,height,name in [(1440,1040,'dark'),(1024,900,'laptop'),(768,1024,'tablet'),(375,844,'mobile')]:
        await page.set_viewport_size(dict(width=width,height=height))
        await page.emulate_media(color_scheme='dark' if name=='dark' else 'light')
        await page.screenshot(path=str(OUT/f'after-{name}.png'),full_page=True)
        if name == 'dark': await check_accessibility(page,'dark-code-review')
        overflow=await page.evaluate('document.documentElement.scrollWidth > innerWidth')
        if overflow: print({'overflow':name,'items':await page.evaluate('[...document.querySelectorAll("body *")].filter(e=>e.getBoundingClientRect().right>innerWidth+1).map(e=>({tag:e.tagName,cls:e.className,width:e.getBoundingClientRect().width,right:e.getBoundingClientRect().right})).slice(0,18)')},flush=True)
        assert not overflow, (name,'horizontal overflow')
        if name=='mobile':
            assert not await page.get_by_text('No hosted runs yet.').is_visible()
            await page.get_by_role('button',name='Session details and history',exact=True).click()
            assert await page.get_by_text('No hosted runs yet.').is_visible()
            await page.get_by_role('button',name='Hide session details',exact=True).click()
            await page.get_by_role('button',name='Review options',exact=True).click()
            options=page.get_by_role('dialog',name='Review options')
            await options.wait_for()
            bounds=await options.bounding_box()
            assert bounds['x'] >= 0 and bounds['x']+bounds['width'] <= width
            await page.keyboard.press('Escape')
            await page.get_by_role('button',name='Session overview',exact=True).click()
            await page.get_by_role('button',name='Close overview',exact=True).click()
    await page.set_viewport_size(dict(width=1440,height=1040)); await page.emulate_media(color_scheme='light')
    await page.set_viewport_size(dict(width=720,height=520))
    assert not await page.evaluate('document.documentElement.scrollWidth > innerWidth'), '200-percent equivalent viewport overflow'
    await page.set_viewport_size(dict(width=1440,height=1040))
    state['runs']=[{'id':'sample','provider':'codex','provider_session_id':'sample-context','stage':'implementing','task':'implement','cwd':state['session']['worktree'],'status':'idle','model':None,'permission_mode':'ask','started_at':'2026-10-30T07:00:00Z','ended_at':None,'error':None,'seq':1}]
    await page.evaluate('(run) => window.__mockWS.onmessage({data:JSON.stringify({type:"run-started",run})})', state['runs'][0])
    await page.get_by_text('The refresh lifecycle is ready for review.',exact=False).wait_for()
    await page.screenshot(path=str(OUT/'after-agent.png'),full_page=True)
    await check_accessibility(page,'agent-activity')
    state['runs']=[]
    await page.goto(url,wait_until='networkidle')
    await page.screenshot(path=str(OUT/'after-inbox.png'),full_page=True)
    await check_accessibility(page,'inbox')
    await page.get_by_role('button',name='Drop Fix session timeout',exact=True).click()
    await page.get_by_role('alertdialog').wait_for()
    await check_accessibility(page,'delete-confirmation')
    await page.screenshot(path=str(OUT/'after-delete.png'),full_page=True)
    await page.get_by_role('button',name='Cancel',exact=True).click()
    await page.get_by_role('button',name='New session',exact=True).click()
    await page.get_by_placeholder('Fix the login timeout').fill('लगइन सुधार')
    assert await page.get_by_role('button',name='Create session',exact=True).is_enabled()
    await page.screenshot(path=str(OUT/'after-new-session.png'),full_page=True)
    await check_accessibility(page,'new-session')
    await page.get_by_role('button',name='Cancel',exact=True).click()
    empty_inbox=True
    await page.reload(wait_until='networkidle')
    await page.get_by_role('button',name='Create your first session',exact=True).wait_for()
    await page.screenshot(path=str(OUT/'after-empty.png'),full_page=True)
    await check_accessibility(page,'empty-inbox')
    assert not errors, errors
    await context.close()
    return {'browser_errors':errors,'screenshots':str(OUT)}

async def main():
    class Handler(SimpleHTTPRequestHandler):
        def do_GET(self):
            if self.path.split('?')[0].startswith('/s/'):
                self.path='/index.html'
            super().do_GET()
        def log_message(self, *args): pass
    servers=[]
    server=ThreadingHTTPServer(('127.0.0.1',0),partial(Handler,directory=str(ROOT/'web/dist')))
    threading.Thread(target=server.serve_forever,daemon=True).start()
    servers.append(server)
    try:
        async with async_playwright() as p:
            browser=await p.chromium.launch(executable_path=ARGS.browser,headless=True,args=['--no-sandbox'])
            print(await capture(browser,server.server_address[1]),flush=True)
            await browser.close()
    finally:
        for server in servers: server.shutdown()

asyncio.run(main())
