// The site's script entry (ADR 0034 §4). Pages read and work without it; it marks the
// document as having JavaScript, and the Vue components of later phases mount from here
// into the placeholders the server wrote.
document.documentElement.classList.replace( 'client-nojs', 'client-js' );
