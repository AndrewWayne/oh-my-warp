export type CapabilityScope =
  | "session.create"
  | "session.read"
  | "session.write"
  | "session.list"
  | "session.delete"
  | "agent.run"
  | "host.read";

export interface Capability {
  v: number;
  hostId: string;
  deviceId: string;
  devicePub: string; // base64url
  scopes: CapabilityScope[];
  iat: string;
  exp: string;
  sig: string; // base64url
}

export type FrameKind = "data" | "resize" | "ack" | "close" | "ping" | "pong";
