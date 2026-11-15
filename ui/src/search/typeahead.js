// The header's search box (ADR 0034 §4; chapter 19 §5.3). The server draws a plain form,
// which searches without script. Where it also wrote the search data, the suggesting
// component (Codex's Lookup, with Vue) is loaded the first time the box is used and
// takes the form's place, so a reader who never searches downloads none of it.

/**
 * Loads the suggesting search box when the header's box is first focused or typed in.
 *
 * @param {Document} doc The document.
 * @param {Function} [load] Loads the component's module; the real import by default.
 */
export function initTypeahead( doc, load ) {
	const form = doc.querySelector( 'form.ts-header__search' );
	const block = doc.getElementById( 'ts-search-data' );
	if ( !form || !block ) {
		return;
	}
	const input = form.querySelector( 'input[type="search"]' );
	if ( !input ) {
		return;
	}
	let data;
	try {
		data = JSON.parse( block.textContent );
	} catch ( e ) {
		return;
	}
	const fetchModule = load || ( () => import( './mount.ts' ) );
	let asked = false;
	const start = () => {
		if ( asked ) {
			return;
		}
		asked = true;
		fetchModule().then(
			( m ) => m.mount( form, data ),
			// The plain form still searches; a later use of the box tries again.
			() => {
				asked = false;
			}
		);
	};
	input.addEventListener( 'focus', start );
	input.addEventListener( 'input', start );
	if ( doc.activeElement === input ) {
		start();
	}
}
