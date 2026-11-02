#!/usr/bin/env python3
"""Production UI checks with synthetic sessions, providers, and diffs.
Requires Python Playwright and a Chromium browser. Build web/dist first.
Optional axe-core path enables a WCAG scan. No actual daemon is contacted.
"""
import argparse, asyncio, copy, json, pathlib, re, shutil, threading
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from functools import partial
from urllib.parse import urlparse, parse_qs
from playwright.async_api import async_playwright, expect

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
DIFFS = {
    'src/auth/session.ts': 'diff --git a/src/auth/session.ts b/src/auth/session.ts\n@@ -1,3 +1,4 @@\n export function refreshSession() {\n-  return expireSession();\n+  scheduleRefresh();\n+  return keepSessionAlive();\n }\n',
    'src/auth/session.test.ts': 'diff --git a/src/auth/session.test.ts b/src/auth/session.test.ts\n@@ -0,0 +1,3 @@\n+test("refreshes before expiry", () => {\n+  expect(refreshSession()).toBe(true);\n+});\n',
}
BLOCKER = 'A timer refresh can race with sign-out. Cancel the timer before clearing credentials.'

def fixture(stage='plan-review'):
    now = '2026-10-29T10:00:00Z'
    docs = [dict(kind=k, path=f'/home/customer/.plantool/sessions/shop/fix-timeout/{k}.md', exists=True, sha='approved123', lines=30, title='Fix session timeout', captured_at=now, progress=[dict(name='Refresh lifecycle',done=1,total=3),dict(name='Verify recovery',done=0,total=2)]) for k in ['research','plan']]
    comments = [dict(id='c1',doc='plan',anchor=dict(line=14,text='- [ ] Refresh credentials before the expiry window',outdated=False),body=BLOCKER,kind='human',type='blocker',author='customer',parent=None,resolved=False,created_at=now,updated_at=None,seq=4)]
    return dict(key='shop/fix-timeout',url_path='/s/shop/fix-timeout',dir='/sessions/shop/fix-timeout',session=dict(repo_slug='shop',slug='fix-timeout',title='Fix session timeout',repo=dict(root='/home/customer/projects/shop',common_dir='/home/customer/projects/shop/.git',branch='main'),worktree='/home/customer/projects/shop/.worktrees/fix-timeout',base='main',difftool='difftool',mirror=False,brief='Customers are signed out while working. Keep active sessions alive and handle expired credentials gracefully.',created_at=now),state=dict(stage=stage,seq=5,comments=comments,docs={},review=None,updated_at=now,activity=[],milestones=[],proposals=[]),docs=docs,runs=[],open_comments=1,workspace_branch='plantool/fix-timeout',step='Plan',handoff=dict(step='Plan',label='Owner approving',action='Review plan'))

def milestone_run():
    return {'id':'m1','provider':'codex','provider_session_id':'m1-context','stage':'implementing','task':'implement','implementation_mode':'step-by-step','milestone_pending':True,'milestones_approved':0,'cwd':'/home/customer/projects/shop/.worktrees/fix-timeout','status':'idle','model':None,'permission_mode':'ask','started_at':'2026-10-30T06:00:00Z','ended_at':None,'error':None,'seq':3}

async def check_accessibility(page, label):
    if not ARGS.axe_script:
        return
    await page.add_script_tag(path=str(ARGS.axe_script))
    violations = await page.evaluate('async () => { const r=await axe.run(document,{runOnly:{type:"tag",values:["wcag2a","wcag2aa","wcag21a","wcag21aa"]}}); return r.violations.map(v=>({id:v.id,impact:v.impact,nodes:v.nodes.map(n=>({target:n.target,summary:n.failureSummary}))})); }')
    print({'surface': label, 'accessibility': violations}, flush=True)
    assert not [issue for issue in violations if issue['impact'] in ['serious', 'critical']], violations

async def is_focused(locator):
    return await locator.evaluate('(e) => e === document.activeElement')

async def capture(browser, port):
    context = await browser.new_context(viewport=dict(width=1440,height=1040), device_scale_factor=1, color_scheme='light')
    await context.add_init_script('''window.WebSocket = class { constructor() { window.__mockWS = this; setTimeout(() => this.onopen?.(), 0); } close() {} };''')
    state = fixture()
    empty_inbox = False
    posts = []
    errors=[]
    page=await context.new_page()
    page.on('pageerror', lambda e: errors.append(str(e)))
    async def route(req):
        u = urlparse(req.request.url); path=u.path; method=req.request.method
        if method != 'GET':
            posts.append((method, path, req.request.post_data_json if req.request.post_data else None))
        response={}
        if path == '/api/sessions' and method == 'POST':
            response={'session':{'url_path':state['url_path']}}
        elif path == '/api/sessions':
            response=[] if empty_inbox else [state]
            for slug,title,stage in ([] if empty_inbox else [('checkout-keyboard','Improve checkout keyboard navigation','research-review'),('receipt-email','Add order receipt emails','implementing'),('search-empty','Refine search empty states','done')]):
                item=copy.deepcopy(state); item['key']=f'shop/{slug}'; item['url_path']=f'/s/shop/{slug}'; item['session']['slug']=slug; item['session']['title']=title; item['state']['stage']=stage; item['state']['comments']=[]; item['open_comments']=0; item['runs']=[]; item['session']['worktree']=f'/home/customer/projects/shop/.worktrees/{slug}'; item['workspace_branch']=f'plantool/{slug}'; response.append(item)
        elif path == '/api/providers': response={'providers':[{'id':'claude','available':False,'models':[]},{'id':'codex','available':True,'version':'1.0','models':[]}]}
        elif path == '/api/repos': response={'repos':[{'root':state['session']['repo']['root'],'repo_slug':'shop','branch':'main','base':'main','sessions':1,'last_used':'2026-10-29T10:00:00Z'}]}
        elif path.endswith('/comments'): response={'seq':5,'comments':state['state']['comments']}
        elif path.endswith('/revisions'): response={'approved':None,'viewed':None,'change':None,'pending':False}
        elif '/runs/sample/events' in path: response={'events':[{'seq':1,'at':'2026-10-30T07:00:00Z','event':{'type':'message','id':'sample-result','role':'assistant','content':'The refresh lifecycle is ready for review. I added cleanup on sign-out and coverage for background-tab recovery. Please review the diff before accepting the implementation.'}}]}
        elif path.endswith('/events'): response={'events':[]}
        elif path.endswith('/milestone/approve'): response={'ok':True,'committed':True,'sha':'5b1f0c9e7a2d4b6c8e0f1a3b5c7d9e1f2a4b6c8d'}
        elif path.endswith('/milestone'): response={'subject':'Refresh session lifecycle','dirty':True,'milestone':1,'pending':True,'live':True}
        elif '/prompt' in path: response={'prompt':'Read the session brief and write the implementation plan.','stage':'plan','session_stage':state['state']['stage']}
        elif '/changes/file' in path:
            file=parse_qs(u.query)['path'][0]
            response={'path':file,'diff':DIFFS[file],'truncated':False}
        elif '/changes' in path:
            scope=parse_qs(u.query).get('scope',['step'])[0]
            response={'tool':'difftool','scope':scope,'base':('3e91b5c2d7a48f09e6b13c7840a9d2f1b6c5e8a0' if scope=='step' else '19a7ce032b456df0a1c4e9b7d608f235ac17e9b2'),'label':('since this milestone started' if scope=='step' else 'in the whole implementation'),'step_available':True,'stat':[{'path':'src/auth/session.ts','added':18,'deleted':4},{'path':'src/auth/session.test.ts','added':32,'deleted':0}],'review':None}
        elif '/docs/' in path:
            k='research' if '/research' in path else 'plan'
            response={'kind':k,'content':PLAN,'sha':'approved123','headings':[],'progress':state['docs'][0]['progress'],'captured_at':'2026-10-29T10:00:00Z','lines':30}
        elif path.endswith('/stage'):
            body=req.request.post_data_json
            state['state']['stage']=body['to']
            if body['to']=='approved': state['state']['plan_revision_pending']=False
            response={'stage':state['state']['stage']}
        elif path.startswith('/api/sessions/shop/fix-timeout'): response=state
        else: response={}
        await req.fulfill(status=200,content_type='application/json',body=json.dumps(response))
    await context.route('**/api/**',route)
    url=f'http://127.0.0.1:{port}'
    spine=page.get_by_role('complementary',name='Session',exact=True)
    notice_heading=spine.get_by_role('status')
    async def stage_post(action):
        async with page.expect_response(lambda response: response.url.endswith('/stage')):
            await action
        return posts[-1][2]

    # Plan review with an open blocker: the Notice offers only an override.
    await page.goto(url+'/s/shop/fix-timeout?tab=plan',wait_until='networkidle')
    await spine.get_by_text('3 waiting on you').wait_for()
    await page.screenshot(path=str(OUT/'after-plan.png'), full_page=True)
    assert await spine.get_by_text('Notice of allowance',exact=True).is_visible()
    assert await notice_heading.inner_text()=='Your turn: approve the plan'
    plan_fig=spine.get_by_role('button',name='FIG. 2 Plan, current',exact=True)
    assert await plan_fig.get_attribute('aria-pressed')=='true'
    assert await plan_fig.locator('xpath=..').get_attribute('aria-current')=='step'
    assert await spine.get_by_text('1 blocker stops approval',exact=True).is_visible()
    assert await page.get_by_role('button',name='Approve plan',exact=True).count()==0
    override=spine.get_by_role('button',name='Approve with override…',exact=True)
    assert await override.is_visible() and await override.is_enabled()
    assert not await page.get_by_role('combobox',name='set stage').is_visible()
    await check_accessibility(page,'plan')
    await spine.get_by_role('button',name='Review blocker',exact=True).click()
    comments_drawer=page.get_by_role('complementary',name='Comments',exact=True)
    await comments_drawer.get_by_text(BLOCKER).first.wait_for()
    assert 'drawer=comments' in page.url
    await page.keyboard.press('Escape')
    await comments_drawer.wait_for(state='detached')
    assert 'drawer=' not in page.url

    # Default provider is unavailable: opening a run must select Codex.
    ask=spine.get_by_role('button',name='Ask agent',exact=True)
    await ask.click()
    await page.get_by_role('dialog',name='Ask agent').get_by_role('button',name='Revise with agent',exact=True).click()
    dialog=page.get_by_role('dialog',name='Plan',exact=True)
    provider=dialog.get_by_role('combobox',name=re.compile('^Provider'))
    await page.wait_for_function("document.querySelector('[role=dialog] select')?.value === 'codex'")
    assert await provider.input_value()=='codex'
    await provider.focus(); await page.keyboard.press('Shift+Tab')
    assert await is_focused(dialog.get_by_role('button',name='Start plan',exact=True))
    await page.keyboard.press('Tab')
    assert await is_focused(provider)
    await page.screenshot(path=str(OUT/'after-run-dialog.png'),full_page=True)
    await dialog.get_by_text('The prompt the agent gets',exact=True).click()
    assert await dialog.get_by_role('button',name='Copy prompt',exact=True).is_visible()
    await check_accessibility(page,'run-dialog')
    await dialog.get_by_role('button',name='Cancel',exact=True).click()
    await dialog.wait_for(state='detached')
    await expect(ask).to_be_focused()

    # Approval always confirms; blockers need a recorded reason.
    await page.keyboard.press('a')
    confirm=page.get_by_role('alertdialog',name='Approve the plan with an override')
    await confirm.wait_for()
    submit=confirm.get_by_role('button',name='Approve with recorded override',exact=True)
    assert await submit.is_disabled()
    assert await confirm.get_by_text('Write a reason first',exact=True).is_visible()
    assert await confirm.get_by_text('1 (1 yours)',exact=True).is_visible()
    await confirm.get_by_role('button',name='Cancel',exact=True).click()
    await confirm.wait_for(state='detached')
    assert state['state']['stage']=='plan-review' and not any(p[1].endswith('/stage') for p in posts)
    await override.click()
    await confirm.get_by_label('Why are you overriding the blockers?').fill('Sign-out cancellation is covered in phase 1.')
    assert await submit.is_enabled()
    body=await stage_post(submit.click())
    assert body=={'to':'approved','override_reason':'Sign-out cancellation is covered in phase 1.','plan_sha':'approved123'}, body
    assert state['state']['stage']=='approved'
    await page.evaluate('window.__mockWS.onmessage({data:JSON.stringify({type:"stage-changed",from:"plan-review",to:"approved"})})')
    await spine.get_by_role('button',name='Start build',exact=True).wait_for()

    # A plan revision gates the build until it is accepted.
    state['state']['plan_revision_pending']=True
    await page.reload(wait_until='networkidle')
    accept_revision=spine.get_by_role('button',name='Accept plan revision',exact=True)
    await accept_revision.wait_for()
    assert await notice_heading.inner_text()=='Your turn: accept the plan revision'
    assert await page.get_by_role('button',name='Start build',exact=True).count()==0
    await accept_revision.click()
    confirm=page.get_by_role('alertdialog',name='Accept the plan revision')
    await confirm.wait_for()
    assert await confirm.get_by_text('plan.md rev approve',exact=True).is_visible()
    body=await stage_post(confirm.get_by_role('button',name='Accept plan revision',exact=True).click())
    assert body=={'to':'approved','plan_sha':'approved123'}, body
    await page.evaluate('window.__mockWS.onmessage({data:JSON.stringify({type:"stage-changed",from:"approved",to:"approved"})})')
    start_build=spine.get_by_role('button',name='Start build',exact=True)
    await start_build.wait_for()
    assert await notice_heading.inner_text()=='Plan approved: start the build'

    # Implementation configuration, opened from the keyboard.
    await start_build.focus(); await page.keyboard.press('Enter')
    dialog=page.get_by_role('dialog',name='Implement',exact=True)
    pace=dialog.get_by_label('Implementation pace')
    assert await pace.input_value()=='all-at-once'
    await page.wait_for_function("document.querySelector('[role=dialog] select')?.value === 'codex'")
    await pace.select_option('step-by-step')
    assert await dialog.get_by_text('One plan phase per run.',exact=False).is_visible()
    await dialog.get_by_role('button',name='Cancel',exact=True).click()
    await dialog.wait_for(state='detached')
    await expect(start_build).to_be_focused()
    await spine.get_by_role('button',name='Build step by step',exact=True).click()
    await dialog.wait_for()
    assert await dialog.get_by_label('Implementation pace').input_value()=='step-by-step'
    await dialog.get_by_role('button',name='Cancel',exact=True).click()
    await dialog.wait_for(state='detached')
    assert not any(p[1].endswith('/runs') for p in posts)

    # Milestone review: commit subject, untick, and approval.
    state['state']['stage']='implementing'
    state['state']['milestones']=[{'key':'Phase 1','status':'pending'},{'key':'Phase 2','status':'pending'}]
    state['runs']=[milestone_run()]
    state['step']='Build'
    await page.reload(wait_until='networkidle')
    commit=spine.get_by_role('button',name='Commit milestone 1 of 2 and continue',exact=True)
    await commit.wait_for()
    assert await spine.get_by_text('Milestone review',exact=True).is_visible()
    assert await notice_heading.inner_text()=='Milestone 1 of 2 is ready'
    subject=spine.get_by_label('Commit subject')
    await page.wait_for_function("document.querySelector('.commit-message')?.value === 'Refresh session lifecycle'")
    await spine.get_by_text('2 files +50 −4',exact=True).wait_for()
    tick=spine.get_by_label('commit this milestone')
    await tick.uncheck()
    assert await spine.get_by_role('button',name='Approve milestone 1 of 2 and continue',exact=True).is_enabled()
    await tick.check()
    await subject.fill('')
    assert await commit.is_disabled()
    assert await spine.get_by_text('Write a commit subject, or untick commit',exact=True).is_visible()
    await subject.fill('Refresh credentials before expiry')
    async with page.expect_response(lambda response: response.url.endswith('/milestone/approve')):
        await commit.click()
    assert posts[-1][2]=={'commit':True,'message':'Refresh credentials before expiry'}, posts[-1]
    await page.get_by_text('Milestone committed as 5b1f0c9e7a2d',exact=False).wait_for()
    await page.get_by_role('complementary',name='Codex · implement').wait_for()
    await page.screenshot(path=str(OUT/'after-milestone.png'),full_page=True)

    # Final review: scope and file switching in Changes.
    state['state']['stage']='implementation-review'
    state['state']['milestones']=[]
    state['runs']=[]
    state['step']='Review'
    state['handoff']={'step':'Review','label':'Owner reviewing','action':'Review comments'}
    await page.goto(url+'/s/shop/fix-timeout?tab=plan',wait_until='networkidle')
    await page.get_by_role('navigation',name='Session documents').get_by_role('button',name='Changes',exact=True).click()
    await page.get_by_role('button',name='Expand src/auth/session.ts').click()
    await page.get_by_text('+  scheduleRefresh();').wait_for()
    await page.get_by_role('button',name='Expand src/auth/session.test.ts').click()
    await page.get_by_text('+test("refreshes before expiry", () => {').wait_for()
    await page.get_by_role('button',name='Collapse src/auth/session.ts').click()
    await page.get_by_text('+  scheduleRefresh();').wait_for(state='detached')
    assert await page.get_by_text('+test("refreshes before expiry", () => {').is_visible()
    await page.get_by_role('button',name='Expand src/auth/session.ts').click()
    await page.get_by_text('+  scheduleRefresh();').wait_for()
    await page.screenshot(path=str(OUT/'after-changes.png'),full_page=True)
    whole=page.get_by_role('tab',name='Whole implementation',exact=True)
    await whole.click()
    await page.get_by_text('19a7ce032b456df0a1c4e9b7d608f235ac17e9b2').wait_for()
    assert await whole.get_attribute('aria-selected')=='true'
    assert await page.get_by_role('tab',name='This milestone',exact=True).get_attribute('aria-selected')=='false'
    if not await page.get_by_text('+  scheduleRefresh();').is_visible():
        await page.get_by_role('button',name='Expand src/auth/session.ts').click()
    await page.get_by_text('+  scheduleRefresh();').wait_for()
    agent_toggle=page.get_by_role('button',name=re.compile(r'^Agent\b'))
    for width,height,name in [(1440,1040,'dark'),(1024,900,'laptop'),(768,1024,'tablet'),(390,844,'mobile')]:
        await page.set_viewport_size(dict(width=width,height=height))
        await page.emulate_media(color_scheme='dark' if name=='dark' else 'light')
        await page.screenshot(path=str(OUT/f'after-{name}.png'),full_page=True)
        if name == 'dark': await check_accessibility(page,'dark-code-review')
        overflow=await page.evaluate('document.documentElement.scrollWidth > innerWidth')
        if overflow: print({'overflow':name,'items':await page.evaluate('[...document.querySelectorAll("body *")].filter(e=>e.getBoundingClientRect().right>innerWidth+1).map(e=>({tag:e.tagName,cls:e.className,width:e.getBoundingClientRect().width,right:e.getBoundingClientRect().right})).slice(0,18)')},flush=True)
        assert not overflow, (name,'horizontal overflow')
        if name=='mobile':
            accept=spine.get_by_role('button',name='Accept and finish',exact=True)
            box=await accept.bounding_box()
            assert await accept.is_visible() and box['height'] >= 44 and box['y']+box['height'] <= height, box
            assert not await ask.is_visible()
            await spine.get_by_role('button',name='Show evidence and actions',exact=True).click()
            await expect(ask).to_be_visible()
            assert await spine.get_by_text('Final review',exact=True).is_visible()
            await spine.get_by_role('button',name='Show less',exact=True).click()
            await expect(ask).to_be_hidden()
            assert not await page.get_by_text('No hosted runs yet.').is_visible()
            await agent_toggle.click()
            await expect(page.get_by_text('No hosted runs yet.')).to_be_visible()
            bounds=await page.get_by_role('complementary',name='Agent runs').bounding_box()
            assert bounds['x'] == 0 and bounds['width'] == width, bounds
            await page.get_by_role('button',name='Close agent run',exact=True).click()
            await expect(page.get_by_text('No hosted runs yet.')).to_be_hidden()
            await expect(agent_toggle).to_be_focused()
            await spine.get_by_role('button',name='Session details',exact=True).click()
            details=page.get_by_role('complementary',name='Session details')
            await details.get_by_role('heading',name='History').wait_for()
            await page.keyboard.press('Escape')
            await details.wait_for(state='detached')
            trigger=page.get_by_role('button',name='Review options')
            assert (await trigger.bounding_box())['height'] >= 44
            await trigger.click()
            options=page.get_by_role('dialog',name='Review options')
            await options.wait_for()
            bounds=await options.bounding_box()
            assert bounds['x'] >= 0 and bounds['x']+bounds['width'] <= width
            await page.keyboard.press('Escape')
            await options.wait_for(state='detached')
    await page.set_viewport_size(dict(width=1440,height=1040)); await page.emulate_media(color_scheme='light')
    await page.set_viewport_size(dict(width=720,height=520))
    assert not await page.evaluate('document.documentElement.scrollWidth > innerWidth'), '200-percent equivalent viewport overflow'
    await page.set_viewport_size(dict(width=1440,height=1040))
    state['runs']=[{'id':'sample','provider':'codex','provider_session_id':'sample-context','stage':'implementing','task':'implement','cwd':state['session']['worktree'],'status':'idle','model':None,'permission_mode':'ask','started_at':'2026-10-30T07:00:00Z','ended_at':None,'error':None,'seq':1}]
    await page.evaluate('(run) => window.__mockWS.onmessage({data:JSON.stringify({type:"run-started",run})})', state['runs'][0])
    await page.get_by_text('The refresh lifecycle is ready for review.',exact=False).wait_for()
    await page.screenshot(path=str(OUT/'after-agent.png'),full_page=True)
    await check_accessibility(page,'agent-activity')

    # Done: accepting the implementation confirms first.
    state['runs']=[]
    await page.goto(url+'/s/shop/fix-timeout?tab=changes',wait_until='networkidle')
    await spine.get_by_role('button',name='Accept and finish',exact=True).click()
    confirm=page.get_by_role('alertdialog',name='Accept the implementation')
    await confirm.wait_for()
    assert await confirm.get_by_text('1 / 5 done',exact=True).is_visible()
    body=await stage_post(confirm.get_by_role('button',name='Accept and finish',exact=True).click())
    assert body=={'to':'done'}, body
    await page.evaluate('window.__mockWS.onmessage({data:JSON.stringify({type:"stage-changed",from:"implementation-review",to:"done"})})')
    await spine.get_by_text('Complete',exact=True).wait_for()
    assert await notice_heading.inner_text()=='Done'
    assert await page.get_by_role('button',name='Accept and finish',exact=True).count()==0
    assert await spine.get_by_role('button',name='FIG. 5 Done, done',exact=True).locator('xpath=..').get_attribute('aria-current')=='step'
    await page.screenshot(path=str(OUT/'after-done.png'),full_page=True)

    await page.goto(url,wait_until='networkidle')
    waiting=page.get_by_role('region',name=re.compile('^Waiting on you'))
    await waiting.get_by_role('link',name='Open Improve checkout keyboard navigation').wait_for()
    assert await waiting.get_by_role('link').count()==2
    assert await page.get_by_role('region',name=re.compile('^Recently done')).get_by_role('link',name='Open Fix session timeout').is_visible()
    await page.screenshot(path=str(OUT/'after-inbox.png'),full_page=True)
    await check_accessibility(page,'inbox')
    await page.keyboard.press('j')
    await expect(waiting.get_by_role('link').first).to_be_focused()
    await page.keyboard.press('j')
    await expect(waiting.get_by_role('link').nth(1)).to_be_focused()
    await page.get_by_role('button',name='More for Fix session timeout',exact=True).click()
    await page.get_by_role('dialog',name='More for Fix session timeout').get_by_role('button',name='Drop session…',exact=True).click()
    drop=page.get_by_role('alertdialog',name='Drop shop/fix-timeout?')
    await drop.wait_for()
    await check_accessibility(page,'delete-confirmation')
    await page.screenshot(path=str(OUT/'after-delete.png'),full_page=True)
    await drop.get_by_role('button',name='Cancel',exact=True).click()
    await drop.wait_for(state='detached')
    assert not any(p[0]=='DELETE' for p in posts), posts
    assert await page.get_by_role('link',name='Open Fix session timeout').is_visible()
    await page.get_by_role('button',name='New session',exact=True).click()
    dialog=page.get_by_role('dialog',name='New session')
    await dialog.get_by_placeholder('Fix the login timeout').fill('लगइन सुधार')
    create=dialog.get_by_role('button',name='Create session',exact=True)
    await expect(create).to_be_enabled()
    await page.screenshot(path=str(OUT/'after-new-session.png'),full_page=True)
    await check_accessibility(page,'new-session')
    async with page.expect_response(lambda response: response.url.endswith('/api/sessions') and response.request.method=='POST'):
        await create.click()
    body=posts[-1][2]
    assert body['title']=='लगइन सुधार' and body['repo']=='/home/customer/projects/shop' and re.fullmatch(r'session-[0-9a-f]{8}',body['slug']), body
    await page.wait_for_url('**/s/shop/fix-timeout')
    empty_inbox=True
    await page.goto(url,wait_until='networkidle')
    first_run=page.get_by_role('region',name='No sessions yet')
    await first_run.get_by_role('button',name='New session',exact=True).wait_for()
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
