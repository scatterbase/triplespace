/**
 * Puts the suggesting search box in place of the header's form (ADR 0034 §4): the same
 * field name, placeholder, accessible name and button, with what the reader has typed
 * so far and the focus kept.
 */
import { createApp } from 'vue';
import { Api } from '../edit/api';
import HeaderSearch from './HeaderSearch.vue';
import type { SearchData } from './suggest';

/**
 * Mounts the component over the form.
 *
 * @param form The header's search form
 * @param data The search data
 */
export function mount( form: HTMLFormElement, data: SearchData ): void {
	const input = form.querySelector<HTMLInputElement>( 'input[type="search"]' );
	const button = form.querySelector<HTMLButtonElement>( 'button[type="submit"]' );
	if ( !input || !form.parentNode ) {
		return;
	}
	const focused = form.ownerDocument.activeElement === input;
	const host = form.ownerDocument.createElement( 'div' );
	host.className = `${ form.className } ts-header__search--live`;
	host.setAttribute( 'role', 'search' );
	createApp( HeaderSearch, {
		api: new Api(),
		data,
		initial: input.value,
		placeholder: input.placeholder,
		label: input.getAttribute( 'aria-label' ) ?? '',
		button: button?.textContent?.trim() ?? '',
	} ).mount( host );
	form.parentNode.replaceChild( host, form );
	// Codex's Lookup draws a text field, whatever type it is asked for.
	const live = host.querySelector<HTMLInputElement>( 'input[name="search"]' );
	if ( live && focused ) {
		live.focus();
		live.setSelectionRange( live.value.length, live.value.length );
		// What was typed while the component loaded is searched for as if typed now.
		if ( live.value.trim() ) {
			live.dispatchEvent( new Event( 'input', { bubbles: true } ) );
		}
	}
}
