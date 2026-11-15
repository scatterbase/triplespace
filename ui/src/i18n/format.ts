/**
 * Interface messages in the components (ADR 0034 §9; chapter 19 §5.6): formatted by the
 * `banana-i18n` library, which reads the banana messages the server reads, so a
 * message's `$1`, `{{PLURAL:…}}` and `{{GENDER:…}}` mean in the browser what they mean
 * on the server.
 *
 * The server hands a component the messages it needs, unexpanded, in the data block
 * beside it; this module loads that set into a formatter for the page's language, once.
 */
import Banana from 'banana-i18n';

const formatters = new WeakMap<Record<string, string>, InstanceType<typeof Banana>>();

/**
 * A message with its parameters, in the language given; `⧼key⧽` when the set has no
 * such message, as MediaWiki shows one.
 *
 * @param messages The messages the server handed the component, by key
 * @param lang The interface language, for plural and gender forms
 * @param key The message key
 * @param params The parameters, `$1` first
 * @return The message
 */
export function format(
	messages: Record<string, string>, lang: string, key: string, params: string[] = [],
): string {
	if ( messages[ key ] === undefined ) {
		return `⧼${ key }⧽`;
	}
	let banana = formatters.get( messages );
	if ( !banana ) {
		// Links are the site's to draw, not a message's: `[[…]]` stays text.
		banana = new Banana( lang || 'en', { messages, wikilinks: false } );
		formatters.set( messages, banana );
	}
	return banana.i18n( key, ...params );
}
