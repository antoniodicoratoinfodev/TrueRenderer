#!/usr/bin/env python3
"""Adversarial framing tests against the actual separate decoder executable."""
from pathlib import Path
import json, struct, subprocess, time

root = Path(__file__).resolve().parents[1]
worker = root / "target/debug/tr-worker"
def packet(kind=1, request_id=1, control=None, body=b"", version=1, length=None):
    payload=json.dumps(control if control is not None else {"source_len":len(body),"max_edge":320}).encode()
    return struct.pack("<4sHHQI",b"TRIP",version,kind,request_id,len(payload) if length is None else length)+payload+body
def run(wire):
    return subprocess.run([str(worker)],input=wire,capture_output=True,timeout=5)
results=[]
for name, wire in [
    ("unknown_version",packet(body=b"x",version=65535)),
    ("oversized_control",packet(body=b"x",length=0xffffffff)),
    ("oversized_source",packet(control={"source_len":2**40,"max_edge":320})),
    ("unknown_control_field",packet(control={"source_len":1,"max_edge":320,"path":"/not/authorized"},body=b"x")),
    ("truncated_header",b"TRIP\x01"),
]:
    output=run(wire)
    assert not output.stdout, (name,output.stdout[:100])
    results.append({"case":name,"passed":True})
# Decoder failure is a bounded error response and the worker accepts the next job.
good=(root/"corpus/01_Studio_cromatico.png").read_bytes()
wire=packet(request_id=1,body=b"invalid")+packet(request_id=2,body=good)
output=run(wire).stdout
magic,version,kind,request_id,size=struct.unpack("<4sHHQI",output[:20])
assert (magic,version,kind,request_id)==(b"TRIP",1,3,1)
assert size<65536
cursor=20+size
magic,version,kind,request_id,size=struct.unpack("<4sHHQI",output[cursor:cursor+20])
assert (magic,version,kind,request_id)==(b"TRIP",1,2,2)
info=json.loads(output[cursor+20:cursor+20+size])
assert len(output)-(cursor+20+size)==info["width"]*info["height"]*16
results.append({"case":"decode_error_then_valid_job_same_process","passed":True})
(root/"reports/worker-protocol.json").write_text(json.dumps({"passed":True,"cases":results},indent=2)+"\n")
print(f"Worker protocol: {len(results)} checks passed")
