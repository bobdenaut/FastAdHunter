import { Card } from '../../components/card';

/**
 * The three absences, stated. Each one is a screen an operator might look for
 * and will not find, and saying why once here is cheaper than the same question
 * arriving three times.
 */
export function NeverCard() {
  return (
    <Card title="What this page will never do" className="health-never">
      <div class="row c3">
        <div>
          <p class="access-title">No message store</p>
          <p class="note">
            There is no diagnostics inbox to accumulate warnings. Everything
            here is a live counter or a current status, read fresh — nothing to
            mark as read and nothing to go stale.
          </p>
        </div>
        <div>
          <p class="access-title">No log viewer</p>
          <p class="note">
            No endpoint serves log files, so none is offered. Logs stay where
            the container puts them.
          </p>
        </div>
        <div>
          <p class="access-title">No red without a reason</p>
          <p class="note">
            <span class="mono">degraded</span> is amber and explained, because
            the box is still answering. A status is only escalated when clients
            would actually notice.
          </p>
        </div>
      </div>
    </Card>
  );
}
