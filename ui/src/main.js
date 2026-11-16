// The site's script entry (ADR 0034 §4). Pages read and work without it; it marks the
// document as having JavaScript (the head script has usually done so already), wires the
// Appearance menu (ADR 0085 §6), the challenge of a registration or login form
// (ADR 0087 §5), a diff page's arrow keys (ADR 0010 §6), the bell's panel
// (ADR 0021 A20), the panes of Special:Preferences (19 §2.6) and the suggestions of the
// header's search box, which load when the box is first used, and loads the editing
// components only on a page that has something the viewer may edit (the server writes an
// edit data block there), so a reader downloads none of Vue. The grid of
// Special:CreateManyEntities and of a Table page (ADR 0106 §1; 0045 §6) loads the same way.
import { initAppearance } from './appearance/menu.js';
import { initBell } from './bell/bell.js';
import { initChallenges } from './challenge/form.js';
import { initDiffNav } from './diffnav/diffnav.js';
import { initPrefTabs } from './prefs/tabs.js';
import { initTypeahead } from './search/typeahead.js';

document.documentElement.classList.replace( 'client-nojs', 'client-js' );
initAppearance( document, window );
initChallenges( document );
initDiffNav( document, window );
initBell( document, window );
initPrefTabs( document, window );
initTypeahead( document );
// An import larger than one run goes on by itself (ADR 0106 A11): the report's form
// that starts the next run is the page's own, and is sent for the editor.
const next = document.querySelector( 'form.ts-createentities__continue' );
if ( next ) {
	const button = next.querySelector( 'button[type="submit"]' );
	if ( button ) {
		button.disabled = true;
	}
	next.submit();
}
if ( document.getElementById( 'ts-edit-data' ) ) {
	import( './edit/index.ts' );
}
if ( document.getElementById( 'ts-sheet-data' ) ) {
	import( './grid/index.ts' );
}
