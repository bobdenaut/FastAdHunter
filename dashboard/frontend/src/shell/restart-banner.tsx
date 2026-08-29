import { useEffect, useState } from 'preact/hooks';
import {
  restartArming,
  subscribeRestartBanner,
  type RestartArming,
} from '../system/restart-banner';
import { Icon } from './icon';

/**
 * Rendered above the routed content on every screen, because a pending restart
 * is not a fact about the Settings page.
 *
 * **There is no Dismiss.** The artboard draws one; the task and the IA both say
 * the banner "holds until a restart is observed via `/health` uptime
 * resetting", and a dismissed banner would leave a boot-only change silently
 * pending with nothing left to say so.
 */
export function RestartBanner() {
  const [arming, setArming] = useState<RestartArming | null>(restartArming);

  useEffect(() => subscribeRestartBanner(setArming), []);

  if (arming === null) return null;

  return (
    <div class="banner warn restart-banner" role="status">
      <Icon name="warning" size={16} className="warning" />
      <div>
        <b>
          {arming.keys.length === 0
            ? 'A saved change needs a restart.'
            : arming.keys.length === 1
              ? 'One saved change needs a restart.'
              : `${arming.keys.length} saved changes need a restart.`}
        </b>{' '}
        {arming.keys.length > 0 && (
          <>
            {arming.keys.map((key, index) => (
              <span key={key}>
                {index > 0 && (index === arming.keys.length - 1 ? ' and ' : ', ')}
                <span class="mono">{key}</span>
              </span>
            ))}{' '}
            {arming.keys.length === 1 ? 'is' : 'are'} written to the
            configuration file and will apply on the next start.
          </>
        )}
        {arming.keys.length === 0 && (
          <>
            It is written to the configuration file and will apply on the next
            start. The keys are not named here because the{' '}
            <span class="mono">config_changed</span> event carries only the
            flag — this browser did not submit the change.
          </>
        )}{' '}
        The running engine is unchanged until then.
        <span class="footnote-line">
          This clears when a <span class="mono">/health</span> reading shows the
          process booted after the change was saved. Nothing polls on its
          behalf: it revalidates on entering Settings, and whenever a page that
          reads <span class="mono">/health</span> takes a reading.
        </span>
      </div>
    </div>
  );
}
