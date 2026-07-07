import { ArrowDownToLine, Check, MonitorSmartphone, X } from "lucide-react";
import type { IncomingTransfer } from "../api/desktop";
import { formatBytes } from "../utils";
import { NeonButton } from "./ui";

/**
 * Receiver-side approval modal. Shown when the backend emits
 * `incoming_transfer_request`. The transfer stays parked on the receiver until
 * the user clicks Accept (continue) or Reject (cancel cleanly, no file).
 */
export function IncomingTransferDialog({
  request,
  busy,
  onAccept,
  onReject,
}: {
  request: IncomingTransfer;
  busy?: boolean;
  onAccept: () => void;
  onReject: () => void;
}) {
  return (
    <div className="modal-backdrop">
      <div
        className="modal glass glass--strong"
        role="dialog"
        aria-modal="true"
        aria-label="Incoming transfer"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="modal__glyph modal__glyph--incoming">
          <ArrowDownToLine size={26} />
        </div>
        <h3 className="modal__title">Incoming file</h3>
        <p className="modal__sub text-muted">
          A device wants to send you a file. Nothing is written until you accept.
        </p>

        <div className="modal__rows">
          <div className="kv">
            <span className="kv__k row" style={{ gap: 6 }}>
              <MonitorSmartphone size={13} /> From
            </span>
            <span className="kv__v">{request.sender}</span>
          </div>
          <div className="kv">
            <span className="kv__k">File</span>
            <span className="kv__v mono">{request.filename}</span>
          </div>
          <div className="kv">
            <span className="kv__k">Size</span>
            <span className="kv__v mono">{formatBytes(request.fileSize)}</span>
          </div>
          {request.checksum ? (
            <div className="kv">
              <span className="kv__k">SHA-256</span>
              <span
                className="kv__v mono"
                style={{ fontSize: 11 }}
                title={request.checksum}
              >
                {request.checksum.slice(0, 24)}…
              </span>
            </div>
          ) : null}
        </div>

        <div className="modal__actions">
          <NeonButton variant="danger" icon={X} onClick={onReject} disabled={busy}>
            Reject
          </NeonButton>
          <NeonButton icon={Check} onClick={onAccept} loading={busy}>
            Accept
          </NeonButton>
        </div>
      </div>
    </div>
  );
}
