#!/usr/bin/env python3
"""Generate the standard UPnP AV service descriptions shipped with Framely."""
from pathlib import Path
from xml.etree.ElementTree import Element,SubElement,tostring
root=Path(__file__).parent
services={
'AVTransport':[
 ('SetAVTransportURI',[('InstanceID','in','ui4'),('CurrentURI','in','string'),('CurrentURIMetaData','in','string')]),
 ('Play',[('InstanceID','in','ui4'),('Speed','in','string')]),('Pause',[('InstanceID','in','ui4')]),('Stop',[('InstanceID','in','ui4')]),
 ('GetTransportInfo',[('InstanceID','in','ui4'),('CurrentTransportState','out','string'),('CurrentTransportStatus','out','string'),('CurrentSpeed','out','string')]),
 ('GetMediaInfo',[('InstanceID','in','ui4')]+[(v,'out',t) for v,t in [('NrTracks','ui4'),('MediaDuration','string'),('CurrentURI','string'),('CurrentURIMetaData','string'),('NextURI','string'),('NextURIMetaData','string'),('PlayMedium','string'),('RecordMedium','string'),('WriteStatus','string')]]),
 ('Seek',[('InstanceID','in','ui4'),('Unit','in','string'),('Target','in','string')]),
 ('GetDeviceCapabilities',[('InstanceID','in','ui4'),('PlayMedia','out','string'),('RecMedia','out','string'),('RecQualityModes','out','string')]),
 ('GetTransportSettings',[('InstanceID','in','ui4'),('PlayMode','out','string'),('RecQualityMode','out','string')]),
 ('GetCurrentTransportActions',[('InstanceID','in','ui4'),('Actions','out','string')]),
 ('GetPositionInfo',[('InstanceID','in','ui4')]+[(v,'out',t) for v,t in [('Track','ui4'),('TrackDuration','string'),('TrackMetaData','string'),('TrackURI','string'),('RelTime','string'),('AbsTime','string'),('RelCount','i4'),('AbsCount','i4')]])],
'RenderingControl':[(k,[('InstanceID','in','ui4'),('Channel','in','string'),(v,d,t)]) for k,v,d,t in [('GetVolume','CurrentVolume','out','ui2'),('SetVolume','DesiredVolume','in','ui2'),('GetMute','CurrentMute','out','boolean'),('SetMute','DesiredMute','in','boolean')]],
'ConnectionManager':[('GetProtocolInfo',[('Source','out','string'),('Sink','out','string')]),('GetCurrentConnectionIDs',[('ConnectionIDs','out','string')]),('GetCurrentConnectionInfo',[('ConnectionID','in','i4')]+[(v,'out',t) for v,t in [('RcsID','i4'),('AVTransportID','i4'),('ProtocolInfo','string'),('PeerConnectionManager','string'),('PeerConnectionID','i4'),('Direction','string'),('Status','string')]])],
}
for service,actions in services.items():
 doc=Element('scpd',xmlns='urn:schemas-upnp-org:service-1-0');version=SubElement(doc,'specVersion');SubElement(version,'major').text='1';SubElement(version,'minor').text='0';actionlist=SubElement(doc,'actionList');variables={}
 for name,args in actions:
  action=SubElement(actionlist,'action');SubElement(action,'name').text=name;arglist=SubElement(action,'argumentList')
  for v,d,t in args:
   arg=SubElement(arglist,'argument');SubElement(arg,'name').text=v;SubElement(arg,'direction').text=d;SubElement(arg,'relatedStateVariable').text='A_ARG_TYPE_'+v;variables[v]=t
 table=SubElement(doc,'serviceStateTable')
 for v,t in variables.items():
  var=SubElement(table,'stateVariable',sendEvents='no');SubElement(var,'name').text='A_ARG_TYPE_'+v;SubElement(var,'dataType').text=t
 for name in (['LastChange'] if service!='ConnectionManager' else ['SourceProtocolInfo','SinkProtocolInfo','CurrentConnectionIDs']):
  var=SubElement(table,'stateVariable',sendEvents='yes');SubElement(var,'name').text=name;SubElement(var,'dataType').text='string'
 (root/(service+'.xml')).write_bytes(tostring(doc,encoding='utf-8',xml_declaration=True))
