// The site's script entry (ADR 0034 §4). Pages read and work without it; it marks the
// document as having JavaScript, and loads the editing components only on a page that
// has something the viewer may edit (the server writes an edit data block there), so a
// reader downloads none of Vue.
document.documentElement.classList.replace( 'client-nojs', 'client-js' );
if ( document.getElementById( 'ts-edit-data' ) ) {
	import( './edit/index.ts' );
}
