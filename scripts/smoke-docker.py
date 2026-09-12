"""End-to-end disposable local Docker deployment through the manager API.
Requires built target/debug/vsm(.exe), bundled .vsm/artifacts/vsm-agent and Docker Linux x64.
Creates a named volume/container then explicitly deletes its own data after successful checks.
"""
import json, os, pathlib, socket, subprocess, time, urllib.request, uuid, shutil
root=pathlib.Path(__file__).resolve().parents[1]
data=root/'.vsm'/('docker-smoke-'+str(uuid.uuid4()))
data.mkdir(parents=True)
env=os.environ.copy();env['VSM_AGENT_BINARY']=str(root/'.vsm/artifacts/vsm-agent')
exe=root/'target/debug'/('vsm.exe' if os.name=='nt' else 'vsm')
shutil.copy2(exe, data / exe.name)
exe=data / exe.name
log=open(data/'manager.log','w')
p=subprocess.Popen([str(exe),'serve','--data',str(data),'--bind','127.0.0.1:4327'],env=env,stdout=log,stderr=log)
try:
 for _ in range(50):
  if (data/'owner.token').exists():break
  time.sleep(.2)
 token=(data/'owner.token').read_text().strip()
 def api(method,path,body=None):
  req=urllib.request.Request('http://127.0.0.1:4327/api/v1/'+path,data=json.dumps(body).encode() if body is not None else None,method=method,headers={'Authorization':'Bearer '+token,'Content-Type':'application/json','Idempotency-Key':str(uuid.uuid4())})
  try:
   with urllib.request.urlopen(req,timeout=650) as r:return json.load(r)
  except urllib.error.HTTPError as e:raise RuntimeError(e.read().decode()) from e
 def wait(op):
  for _ in range(600):
   status=api('GET','operations/'+op['id'])
   if status['status']=='succeeded':return status
   if status['status'] in ('failed','interrupted'):raise RuntimeError(status['message'])
   time.sleep(2)
  raise TimeoutError('Operation timed out')
 def action(name,body=None):return wait(api('POST','servers/'+sid+'/actions/'+name,body or {}))
 time.sleep(.5)
 account=api('POST','accounts',{'id':'','name':'Docker smoke','provider':'docker','credential_ref':'','settings':{'game_port':42429}})
 spec={'name':'docker-smoke','account_id':account['id'],'region':'local','size':'docker','game_version':'1.22.0','ssh_public_key':'','ssh_private_key':'','admin_cidr':'','existing_host':'','ssh_user':''}
 plan=api('POST','plans',spec);sid=plan['id'];container='vsm-'+sid
 print('Building and deploying '+container,flush=True)
 wait(api('POST','deployments',{'plan_id':sid}))
 print('Starting game',flush=True);action('start')
 for _ in range(120):
  status=api('GET','servers/'+sid+'/agent/status')
  if status['state']=='running':break
  time.sleep(2)
 else:raise RuntimeError('Game not ready')
 with socket.create_connection(('127.0.0.1',42429),timeout=5):pass
 inspect=json.loads(subprocess.check_output(['docker','inspect',container]))[0]
 assert set(inspect['HostConfig']['PortBindings'])=={'42420/tcp','42420/udp'}
 assert all(m['Destination']!='/var/run/docker.sock' for m in inspect['Mounts'])
 assert subprocess.run(['docker','exec','--user','vintagestory',container,'test','-r','/var/lib/vsm/agent.token']).returncode!=0
 print('Testing console, backup, restore and retained data',flush=True)
 action('console',{'command':'stats'});action('backup');action('stop')
 points=api('GET','servers/'+sid+'/agent/backups')['recovery_points']
 action('restore',{'id':points[-1]['name']})
 action('delete',{'confirm_name':'docker-smoke'})
 subprocess.run(['docker','volume','inspect',container+'-data'],check=True,stdout=subprocess.DEVNULL)
 # API can complete interrupted teardown and explicit data deletion using the same owned name.
 action('delete',{'delete_data':True,'confirm_name':'docker-smoke'})
 assert subprocess.run(['docker','volume','inspect',container+'-data'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL).returncode!=0
 print(json.dumps({'result':'passed','server_id':sid,'checks':['manager plan/apply','official game install','custom-port connectivity','unprivileged game','private agent','console','backup/restore','retain data','explicit owned cleanup']}),flush=True)
finally:
 p.terminate();p.wait(timeout=20);log.close()
