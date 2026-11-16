// The challenge on a form (ADR 0087 §5): a form the server marks `data-ts-challenge` holds
// the challenge in `tschallengeid` and an empty `tschallengesolution`. Its solving starts
// as soon as the page loads, in a Web Worker, with its progress in the form's polite live
// region; the solution is written into the field. The submit button waits only if the
// person is faster than the solver: a submission before then is held and sent when the
// solution comes. Without a worker it solves on the page's thread, in slices.
import { solve } from './solver.js';

/**
 * @param {HTMLFormElement} form
 * @param {function(): (Worker|null)} makeWorker
 */
export function initChallengeForm( form, makeWorker ) {
	const idField = /** @type {HTMLInputElement|null} */ ( form.querySelector( 'input[name="tschallengeid"]' ) );
	const solutionField = /** @type {HTMLInputElement|null} */ ( form.querySelector( 'input[name="tschallengesolution"]' ) );
	const status = /** @type {HTMLElement|null} */ ( form.querySelector( '[data-ts-challenge-status]' ) );
	if ( !idField || !solutionField ) {
		return;
	}
	const bits = Number( idField.dataset.tsChallengeBits );
	const say = ( key ) => {
		if ( status && status.dataset[ key ] ) {
			status.textContent = status.dataset[ key ];
		}
	};
	let solved = solutionField.value !== '';
	let held = null;
	const done = ( solution ) => {
		if ( solution === null || solution === undefined ) {
			say( 'failed' );
			return;
		}
		solutionField.value = String( solution );
		solved = true;
		say( 'done' );
		if ( held ) {
			const submitter = held;
			held = null;
			form.requestSubmit( submitter instanceof HTMLButtonElement ? submitter : undefined );
		}
	};
	form.addEventListener( 'submit', ( e ) => {
		if ( solved ) {
			return;
		}
		e.preventDefault();
		held = /** @type {SubmitEvent} */ ( e ).submitter || true;
		say( 'waiting' );
	} );
	if ( solved || !Number.isFinite( bits ) ) {
		return;
	}
	say( 'working' );
	let worker = null;
	try {
		worker = makeWorker();
	} catch ( err ) {
		worker = null;
	}
	if ( worker ) {
		worker.onmessage = ( e ) => {
			if ( e.data && 'solution' in e.data ) {
				done( e.data.solution );
				worker.terminate();
			}
		};
		worker.onerror = () => {
			worker.terminate();
			done( solve( idField.value, bits ) );
		};
		worker.postMessage( { challenge: idField.value, bits } );
	} else {
		setTimeout( () => done( solve( idField.value, bits ) ), 0 );
	}
}

/**
 * Wires every challenge form on the page.
 *
 * @param {Document} doc
 */
export function initChallenges( doc ) {
	for ( const form of doc.querySelectorAll( 'form[data-ts-challenge]' ) ) {
		initChallengeForm(
			/** @type {HTMLFormElement} */ ( form ),
			() => new Worker( new URL( './worker.js', import.meta.url ), { type: 'module' } )
		);
	}
}
