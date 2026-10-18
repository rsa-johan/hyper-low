#protocol=can
#inc=common
#id=standard

type Connection:
.connected 1
.disconnected 0

def {0} StatusId:
.status_id .. u8

def {1} ConnectionStatus:
.connected 0 Connection

def {2,3} TemperatureError:
[#2]
.temp0 0 bool
.temp1 1 bool
.temp2 2 bool
.temp3 3 bool
.temp4 4 bool
.temp5 5 bool
.temp6 6 bool
.temp7 7 bool

[#3]
.temp8 0 bool

def {0} TemperatureSensorValue:
.curr_temp .. u8

spec: -- file specific constants, standard types only
.node_id 0x02 u8 -- Coproc node id

frame CoprocStatus:
.size '4
.filter Node(spec.node_id) | Kind(Status) | Method(0)
.ConnectionStatus
.TemperatureError

frame TemperatureValue:
.size '1
.filter Node(spec.node_id) | Kind(Status) | Method(1)
.TemperatureSensorValue

-- functions: the caller builds and sends the request; `.rsp` filters the reply

fn GetTemperature(): TemperatureError
.rsp Node(spec.node_id) | Kind(GetResponse) | Method(1) | Tag(0xA)

fn GetConnectionState(): ConnectionStatus
.rsp Node(spec.node_id) | Kind(GetResponse) | Method(2)

fn SetFanSpeed(value: u16): Void -- Void: no response frame
