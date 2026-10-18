#protocol=CAN
-- shared enums, constants and filters; include with `#inc=common`

type NodeId:
.MC 0x01
.Coproc 0x02
.ConsoleRadio 0x03

type Priority:
.High 0x00
.Medium 0x01
.Low 0x02

type MsgType:
.Get 0x00
.GetResponse 0x01
.Set 0x02
.Status 0x03
.Ack 0x04

-- example 11-bit id layout: [10:8] node | [7:6] priority | [5:3] msg type | [2:0] method
filter Node(node: NodeId):
.id 8..11 node

filter Prio(p: Priority):
.id 6..8 p

filter Kind(kind: MsgType):
.id 3..6 kind

filter Method(n: u8):
.id 0..3 n

-- payload tag in the low nibble of data byte 0
filter Tag(tag: u8):
.data {0} 0..4 tag
