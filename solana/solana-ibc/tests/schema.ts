import { BorshSchema, Unit } from "borsher";

const tracePathSchema = BorshSchema.Vec(
  BorshSchema.Struct({
    port_id: BorshSchema.String,
    channel_id: BorshSchema.String,
  })
);

const packetDataSchema = BorshSchema.Struct({
  token: BorshSchema.Struct({
    denom: BorshSchema.Struct({
      trace_path: tracePathSchema,
      base_denom: BorshSchema.String,
    }),
    amount: BorshSchema.Array(BorshSchema.u8, 32),
  }),
  sender: BorshSchema.String,
  receiver: BorshSchema.String,
  memo: BorshSchema.String,
});

const timeoutHeightSchema = BorshSchema.Enum({
  Never: BorshSchema.Unit,
  At: BorshSchema.Struct({
    revision_number: BorshSchema.u64,
    revision_height: BorshSchema.u64,
  }),
});
const timeoutTimestampSchema = BorshSchema.Struct({
  time: BorshSchema.u64,
});

export const msgTransferSchema = BorshSchema.Struct({
  port_id_on_a: BorshSchema.String,
  chan_id_on_a: BorshSchema.String,
  packet_data: packetDataSchema,
  timeout_height_on_b: timeoutHeightSchema,
  timeout_timestamp_on_b: timeoutTimestampSchema,
});

export const instructionSchema = BorshSchema.Struct({
  discriminator: BorshSchema.Array(BorshSchema.u8, 8),
  hashed_base_denom: BorshSchema.Array(BorshSchema.u8, 32),
  msg: msgTransferSchema,
});
