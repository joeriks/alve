/** Illustrative design types. Not runtime validators or a complete sync schema. */
export type ID = string;
export type ISODate = string;
export type ISOInstant = string;
export type Decimal = string;

export type FactValue =
  | { type: "text"; value: string }
  | { type: "boolean"; value: boolean }
  | { type: "date"; value: ISODate }
  | { type: "datetime"; value: ISOInstant; timeZone: string }
  | { type: "money"; amount: Decimal; currency: string }
  | { type: "quantity"; amount: Decimal; unit: string };

export interface Provenance {
  origin: "user" | "import" | "ai_proposal";
  verification: "unconfirmed" | "user_confirmed";
  referenceIds: ID[];
}

export interface Fact extends Provenance {
  id: ID;
  key: string;
  label: string;
  value: FactValue | null;
  precision: "exact" | "approximate" | "estimated";
  observedAt?: ISOInstant;
}

export interface NodeBase {
  id: ID;
  vaultId: ID;
  revisionId: ID;
  titleTemplate: string;
  facts: Fact[];
  referenceIds: ID[];
  tags: string[];
  createdAt: ISOInstant;
  updatedAt: ISOInstant;
}

export interface MemoryNode extends NodeBase, Provenance {
  type: "memory";
  kind: "decision" | "preference" | "insight" | "commitment" | "record";
  body:
    | { format: "paragraph"; text: string }
    | { format: "bullets"; items: string[] };
  status: "active" | "archived" | "superseded";
  validFrom?: ISODate;
  validUntil?: ISODate;
  reviewAt?: ISODate;
}

export interface ContextNode extends NodeBase {
  type: "project" | "person" | "event" | "document" | "conversation";
  description?: string;
  // Specialized event/document/conversation payloads require additional schemas.
}

export type Node = MemoryNode | ContextNode;

export interface Relation extends Provenance {
  id: ID;
  vaultId: ID;
  revisionId: ID;
  fromId: ID;
  toId: ID;
  type: "belongs_to" | "based_on" | "fulfills"
      | "related_to" | "supersedes" | "contradicts";
  createdAt: ISOInstant;
}

export interface Reference {
  id: ID;
  vaultId: ID;
  title: string;
  target:
    | { type: "url"; url: string }
    | { type: "attachment"; attachmentId: ID }
    | { type: "node"; nodeId: ID; revisionId: ID }
    | { type: "message"; conversationId: ID; messageId: ID };
  locator?: string;
  accessedAt?: ISOInstant;
}

export interface MemoryGraphExport {
  schemaVersion: "0.1-design";
  vaultId: ID;
  nodes: Node[];
  relations: Relation[];
  references: Reference[];
}
