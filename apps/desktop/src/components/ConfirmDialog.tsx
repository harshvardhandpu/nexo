import { FileUp, Send, X } from "lucide-react";
import type { TransferRequest } from "../api/desktop";
import { formatBytes } from "../utils";
import { NeonButton } from "./ui";

/**
 * Mandatory AirDrop confirmation modal. Shown whenever the backend emits a
 * `transfer_request_created` event. No transfer starts until the user clicks
 * Send (approve); Cancel rejects the pending request.
 */
export function ConfirmDialog({
  request,
  busy,
  onApprove,
  onReject,
}: {
  request: TransferRequest;
  busy?: boolean;
  onApprove: () => void;
  onReject: () => void;
}) {
  return (
    <div className="modal-backdrop" onClick={busy ? undefined : onReject}>
      <div
        className="modal glass glass--strong"
        role="dialog"
        aria-modal="true"
        aria-label="Confirm transfer"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="modal__glyph">
          <FileUp size={26} />
        </div>
        <h3 className="modal__title">Device found</h3>
        <p className="modal__sub text-muted">
          Confirm before Nexo sends this file. Nothing is transferred until you
          approve.
        </p>

        <div className="modal__rows">
          <div className="kv">
            <span className="kv__k">Device</span>
            <span className="kv__v">{request.peerDisplayName}</span>
          </div>
          <div className="kv">
            <span className="kv__k">Endpoint</span>
            <span className="kv__v mono">{request.peerAddress}</span>
          </div>
          <div className="kv">
            <span className="kv__k">File</span>
            <span className="kv__v mono">{request.fileName}</span>
          </div>
          <div className="kv">
            <span className="kv__k">Size</span>
            <span className="kv__v mono">{formatBytes(request.fileSize)}</span>
          </div>
        </div>

        <div className="modal__actions">
          <NeonButton variant="ghost" icon={X} onClick={onReject} disabled={busy}>
            Cancel
          </NeonButton>
          <NeonButton icon={Send} onClick={onApprove} loading={busy}>
            Send
          </NeonButton>
        </div>
      </div>
    </div>
  );
}
