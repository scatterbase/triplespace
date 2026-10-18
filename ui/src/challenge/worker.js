// The Web Worker that solves a challenge off the page's thread (ADR 0087 §5), so the form
// stays responsive while the person types. It takes `{challenge, bits}` and answers
// `{progress}` now and then and `{solution}` once.
import { solve } from './solver.js';

self.onmessage = ( e ) => {
	const { challenge, bits } = e.data || {};
	if ( typeof challenge !== 'string' || typeof bits !== 'number' ) {
		return;
	}
	const solution = solve( challenge, bits, ( tried ) => {
		self.postMessage( { progress: tried } );
	} );
	self.postMessage( { solution } );
};
