/**
 * The manage page's import (D51 §9): the picked file comes in as base64, is
 * read by the core in memory, previewed against the first vault, then saved
 * and synced. The file is never written anywhere; a preview is dropped when
 * the vault locks. No master password: importing only adds items.
 */
import type {
  ImportColumnsView,
  ImportDetected,
  ImportField,
  ImportMapping,
  ImportPreviewView,
} from "../shared/messages.ts";
import { fromBase64 } from "../shared/bytes.ts";
import type { Core, CoreFunctions } from "./core.ts";
import { errorCode } from "./core.ts";
import { ServiceError } from "./errors.ts";
import type { Platform } from "./platform.ts";

/**
 * The largest file the page sends: its base64 must fit in one extension
 * message (64 MiB in Chrome). The core's own limit is 50 MB (D51 §7).
 */
export const MAX_IMPORT_BYTES = 32 * 1024 * 1024;

export const IMPORT_FIELDS: readonly ImportField[] = [
  "title",
  "url",
  "username",
  "password",
  "notes",
  "totp",
  "folder",
  "favorite",
  "tags",
];

interface ImporterDeps {
  core: () => Core;
  fns: CoreFunctions;
  platform: Platform;
  touch: () => Promise<void>;
  /** After saving: keep the cache and sync. */
  saved: () => Promise<void>;
}

interface PreviewJson {
  logins: number;
  notes: number;
  cards: number;
  identities: number;
  folders: number;
  trashed: number;
  left_out: [string, number][];
}

export class Importer {
  constructor(private readonly deps: ImporterDeps) {}

  /** The format the file looks like and, for a CSV, its columns. */
  detect(name: string, file: string): ImportDetected {
    const bytes = this.bytes(file);
    const found = this.deps.fns.detectImport(name, bytes);
    const detected = found
      ? (JSON.parse(found) as { format: string; needs_password: boolean })
      : null;
    let columns: ImportColumnsView | null = null;
    if (detected?.format === "csv") {
      columns = this.call(() => {
        const json = JSON.parse(this.deps.fns.importColumns(bytes)) as {
          headers: string[];
          suggested: Record<string, number | null>;
          rows: number;
        };
        return {
          headers: json.headers,
          suggested: mapping(json.suggested),
          rows: json.rows,
        };
      });
    }
    bytes.fill(0);
    return {
      format: detected?.format ?? null,
      needsPassword: detected?.needs_password ?? false,
      columns,
    };
  }

  async prepare(
    file: string,
    format: string,
    password: string,
    keyFile: string,
    columns: ImportMapping | null,
  ): Promise<ImportPreviewView> {
    await this.deps.touch();
    const bytes = this.bytes(file);
    const key = keyFile === "" ? new Uint8Array() : this.bytes(keyFile);
    try {
      const json = this.call(() =>
        this.deps
          .core()
          .prepareImport(
            "",
            format,
            bytes,
            password,
            key,
            columns ? JSON.stringify(columns) : "",
            this.deps.platform.now(),
          ),
      );
      return preview(json);
    } finally {
      bytes.fill(0);
      key.fill(0);
    }
  }

  async finish(): Promise<ImportPreviewView> {
    await this.deps.touch();
    const json = this.call(() =>
      this.deps.core().finishImport(this.deps.platform.now()),
    );
    await this.deps.saved();
    return preview(json);
  }

  cancel(): void {
    this.deps.core().cancelImport();
  }

  private bytes(file: string): Uint8Array {
    // Base64 is 4 characters for every 3 bytes.
    if (file.length > Math.ceil(MAX_IMPORT_BYTES / 3) * 4) {
      throw new ServiceError("import_too_large");
    }
    try {
      return fromBase64(file);
    } catch {
      throw new ServiceError("import_unreadable");
    }
  }

  /** A core call whose error becomes a ServiceError with its code. */
  private call<T>(fn: () => T): T {
    try {
      return fn();
    } catch (error) {
      throw new ServiceError(errorCode(error));
    }
  }
}

function mapping(raw: Record<string, number | null>): ImportMapping {
  const out: ImportMapping = {};
  for (const field of IMPORT_FIELDS) {
    const value = raw[field];
    out[field] = typeof value === "number" ? value : null;
  }
  return out;
}

function preview(json: string): ImportPreviewView {
  const p = JSON.parse(json) as PreviewJson;
  return {
    logins: p.logins,
    notes: p.notes,
    cards: p.cards,
    identities: p.identities,
    folders: p.folders,
    trashed: p.trashed,
    leftOut: p.left_out.map(([reason, count]) => ({ reason, count })),
  };
}
