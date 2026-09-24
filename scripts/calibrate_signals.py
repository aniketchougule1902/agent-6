"""Calibrate real, versioned signal outcomes with purged train/calibration/test partitions."""
from pathlib import Path
import json, time, hashlib, math, os, sys
ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/'data'/'calibration-status.json'
def write(path,value):
 path.parent.mkdir(parents=True,exist_ok=True);temp=path.with_suffix('.tmp');temp.write_text(json.dumps(value,allow_nan=False),encoding='utf-8');os.replace(temp,path)
def main():
 starts={};ends={};outages=[]
 journal=Path(os.environ.get('A6_JOURNAL_PATH',ROOT/'data'/'journal.jsonl'))
 if journal.exists():
  for line in journal.open(encoding='utf-8'):
   try:e=json.loads(line)
   except ValueError:continue
   if e.get('event_type') in ('feed_stale','feed_disconnected'):outages.append(e.get('ts_ms',0))
   s=e.get('signal') or {};sid=s.get('id')
   if e.get('event_type')=='signal' and 'strategy:a6-live-v3' in s.get('reasons',[]) :
    raw=next((x.split(':',1)[1] for x in s.get('reasons',[]) if x.startswith('raw-quality:')),None)
    if raw is not None:
     try:s['confidence']=float(raw)
     except ValueError:continue
     if math.isfinite(s['confidence']) and 0<=s['confidence']<=1:starts[sid]=s
   if e.get('event_type') in ('tp2','stop_loss','expired') and sid not in ends:ends[sid]=e
 rows=sorted([(s,ends[k]) for k,s in starts.items() if k in ends and ends[k].get('signal',{}).get('observed_exit_price') is not None and not any(s['created_at_ms']<=t<=ends[k]['ts_ms'] for t in outages)],key=lambda r:r[0]['created_at_ms'])
 status={'state':'collecting','samples':len(rows),'required':500,'updated_ms':int(time.time()*1000),'reason':'Need 500 completed live-v3 signal outcomes; demos and old model scores excluded'}
 if len(rows)<500:write(OUT,status);print(json.dumps(status));return
 import numpy as np
 from sklearn.linear_model import LogisticRegression
 from sklearn.metrics import brier_score_loss
 sys.path.insert(0,str(ROOT/'research'))
 from agent6_research.calibration import expected_calibration_error
 n=len(rows);i=n//2;j=3*n//4
 train=[r for r in rows[:i] if r[1]['ts_ms']<rows[i][0]['created_at_ms']]
 cal=[r for r in rows[i:j] if r[1]['ts_ms']<rows[j][0]['created_at_ms']];test=rows[j:]
 def xy(part):return np.array([[r[0]['confidence']] for r in part]),np.array([int(r[1]['event_type']=='tp2') for r in part])
 for part in (train,cal,test):
  _,y=xy(part)
  if len(part)<50 or len(set(y))<2 or min(sum(y),len(y)-sum(y))<20:
   status.update(state='collecting',reason='Need both outcome classes and sufficient purged partition sizes');write(OUT,status);return
 x,y=xy(train);model=LogisticRegression(random_state=6).fit(x,y)
 cx,cy=xy(cal);calibrator=LogisticRegression(random_state=6).fit(model.decision_function(cx).reshape(-1,1),cy)
 tx,ty=xy(test);prob=calibrator.predict_proba(model.decision_function(tx).reshape(-1,1))[:,1]
 brier=float(brier_score_loss(ty,prob));baseline=float(brier_score_loss(ty,np.full(len(ty),float(y.mean()))));ece=float(expected_calibration_error(prob,ty))
 metrics={'brier':brier,'ece':ece,'baseline_brier':baseline,'independent_test_samples':len(test),'train_samples':len(train),'calibration_samples':len(cal)}
 approved=brier<baseline and ece<=.08
 status.update(state='validated' if approved else 'rejected',reason='Independent chronological test passed' if approved else 'Independent test does not beat baseline with ECE <= 0.08',metrics=metrics)
 if approved:
  payload={'model_version':'live-v3-'+str(int(time.time())),'intercept':float(model.intercept_[0]),'features':[{'name':'quality_score','mean':0.0,'scale':1.0,'weight':float(model.coef_[0,0])}],'calibration':{'method':'platt','a':float(calibrator.coef_[0,0]),'b':float(calibrator.intercept_[0])},'abstention':{'threshold':.5,'min_coverage':0.0,'validation_utility':0.0},'metrics':metrics}
  raw=json.dumps(payload,sort_keys=True,separators=(',',':'));manifest={'schema_version':1,'model_family':'platt_live_signal_outcome','feature_schema_version':'a6.live.signals.v3','training_data_id':'live-journal:'+hashlib.sha256(journal.read_bytes()).hexdigest(),'code_revision':'a6-live-v3','created_at_ms':int(time.time()*1000),'artifact_sha256':hashlib.sha256(raw.encode()).hexdigest(),'metrics':metrics}
  write(Path(os.environ.get('A6_MODEL_PATH',ROOT/'data'/'calibrated_model.json')),{'manifest':manifest,'model_raw':raw,'model':payload})
 write(OUT,status);print(json.dumps(status))
if __name__=='__main__':main()
