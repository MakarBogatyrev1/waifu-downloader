import json, os, pathlib, subprocess, tempfile, time, urllib.request, urllib.error, sqlite3, concurrent.futures
EXE=pathlib.Path(os.environ.get('WAIFU_SYNC_TEST_EXE',str(pathlib.Path(__file__).resolve().parent/'target/debug'/('waifu-sync.exe' if os.name=='nt' else 'waifu-sync'))))
base='http://127.0.0.1:18787'
def call(path,body=None,token=None,expected=200):
    headers={'Content-Type':'application/json'}
    if token: headers['Authorization']='Bearer '+token
    req=urllib.request.Request(base+path,data=json.dumps(body).encode() if body is not None else None,headers=headers)
    try:
        with urllib.request.urlopen(req,timeout=30) as r: status,data=r.status,r.read()
    except urllib.error.HTTPError as e: status,data=e.code,e.read()
    assert status==expected,(path,status,data[:200])
    if not data: return None
    try: return json.loads(data)
    except json.JSONDecodeError: return {'raw':data.decode(errors='replace')}
with tempfile.TemporaryDirectory(prefix='waifu-sync-test-') as temp:
    db=pathlib.Path(temp)/'test.sqlite3'
    env=dict(os.environ,DATABASE_PATH=str(db),BIND_ADDRESS='127.0.0.1:18787')
    def start():
        p=subprocess.Popen([str(EXE)],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.PIPE,creationflags=getattr(subprocess,'CREATE_NO_WINDOW',0))
        for _ in range(200):
            try: call('/health');return p
            except (OSError,AssertionError):
                if p.poll() is not None: raise RuntimeError(p.stderr.read().decode())
                time.sleep(.05)
        raise RuntimeError('start timeout')
    p=start()
    try:
        call('/state',expected=401)
        a=call('/auth/register',{'username':'Alice','password':'test-password-strong'})
        b=call('/auth/register',{'username':'bob','password':'other-password-strong'})
        at,bt=a['token'],b['token']
        assert a['username']=='alice'
        call('/auth/login',{'username':'alice','password':'wrong-password'},expected=401)
        call('/auth/register',{'username':'alice','password':'test-password-strong'},expected=409)
        aa=call('/auth/login',{'username':'alice','password':'test-password-strong'})['token']
        picture={'key':'https://cdn.waifu.im/test.png','source':None,'artist':'Test','mode':'SFW','tag':'waifu'}
        def op(body,token=at,expected=200):return call('/op',body,token,expected)
        s=op({'op':'favorite_put','picture':picture});assert len(s['favorites'])==1
        s=op({'op':'favorite_put','picture':picture});assert len(s['favorites'])==1
        s=op({'op':'album_create','name':'My album'});aid=s['albums'][0]['id']
        op({'op':'album_add','id':aid,'key':picture['key']})
        op({'op':'album_rename','id':aid,'name':'Private'},bt,400)
        op({'op':'album_add','id':aid,'key':picture['key']},bt,400)
        assert call('/state',token=bt)['albums']==[]
        op({'op':'album_rename','id':aid,'name':'Renamed'})
        prefs={'theme':'dark','accent':[50,100,200],'system_accent':False}
        op({'op':'appearance','appearance':prefs})
        assert call('/state',token=aa)['appearance']==prefs
        def add(i):return op({'op':'favorite_put','picture':dict(picture,key=f'https://cdn.waifu.im/{i}.png')},aa)
        with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:list(pool.map(add,range(12)))
        assert len(call('/state',token=at)['favorites'])==13
        s=op({'op':'favorite_remove','key':picture['key']});assert s['albums'][0]['items']==[]
        op({'op':'appearance','appearance':dict(prefs,theme='invalid')},expected=400)
        call('/op',{'op':'album_create','name':'x'*20000},at,413)
        con=sqlite3.connect(db)
        hashes=con.execute('select password from users').fetchall();assert all(h[0].startswith('$argon2id$') for h in hashes)
        assert con.execute('select count(*) from sessions where hash=?',(at,)).fetchone()[0]==0
        con.close()
        call('/auth/logout',{},at)
        call('/state',token=at,expected=401)
        assert len(call('/state',token=aa)['favorites'])==12
        p.terminate();p.wait(timeout=10);p=start()
        assert len(call('/state',token=aa)['favorites'])==12
        call('/auth/login',{'username':'bob','password':'wrong-password'},expected=401)
        for _ in range(9):call('/auth/login',{'username':'bob','password':'wrong-password'},expected=401)
        call('/auth/login',{'username':'bob','password':'wrong-password'},expected=429)
        print('PASS: registration, login, errors, account isolation, idempotence, albums, cross-session appearance, concurrent updates, bounds, Argon2id, hashed sessions, logout revocation, restart persistence, throttling')
    finally:
        if p.poll() is None:p.terminate();p.wait(timeout=10)
