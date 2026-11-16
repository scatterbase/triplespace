<template>
	<form
		class="ts-terms-editor"
		@submit.prevent="save"
		@keydown.esc="cancel">
		<p class="ts-terms-editor__language">
			{{ t( 'ts-edit-in-language', languageName ) }}
		</p>
		<CdxField>
			<template #label>
				{{ t( 'ts-edit-label' ) }}
			</template>
			<CdxTextInput
				ref="first"
				v-model="label"
				:lang="data.lang"
				:dir="data.dir"
				:disabled="saving"
			/>
		</CdxField>
		<CdxField>
			<template #label>
				{{ t( 'ts-edit-description' ) }}
			</template>
			<CdxTextInput
				v-model="description"
				:lang="data.lang"
				:dir="data.dir"
				:disabled="saving"
			/>
		</CdxField>
		<CdxField>
			<template #label>
				{{ t( 'ts-edit-aliases' ) }}
			</template>
			<template #help-text>
				{{ t( 'ts-edit-aliases-help' ) }}
			</template>
			<CdxTextArea
				v-model="aliases"
				:lang="data.lang"
				:dir="data.dir"
				:disabled="saving"
				autosize
			/>
		</CdxField>
		<CdxMessage v-if="problem" type="error">
			{{ problem }}
		</CdxMessage>
		<div class="ts-terms-editor__actions">
			<CdxButton
				type="submit"
				action="progressive"
				weight="primary"
				:disabled="saving"
			>
				{{ saving ? t( 'ts-edit-saving' ) : t( 'ts-edit-save' ) }}
			</CdxButton>
			<CdxButton
				v-if="conflict"
				type="button"
				:disabled="saving"
				@click="emit( 'restart' )"
			>
				{{ t( 'ts-edit-reload' ) }}
			</CdxButton>
			<CdxButton
				type="button"
				:disabled="saving"
				@click="cancel">
				{{ t( 'ts-edit-cancel' ) }}
			</CdxButton>
		</div>
	</form>
</template>

<script setup lang="ts">
import { onMounted, ref } from 'vue';
import { CdxButton, CdxField, CdxMessage, CdxTextArea, CdxTextInput } from '@wikimedia/codex';
import { ApiError } from './api';
import type { Api } from './api';
import { msg } from './data';
import type { EditData } from './data';
import { termsEdit } from './terms';
import type { Terms } from './terms';

const props = defineProps<{
	data: EditData;
	api: Api;
	initial: Terms;
	lastrevid: number;
}>();

const emit = defineEmits<{
	saved: [ terms: Terms ];
	cancel: [];
	restart: [];
}>();

const t = ( key: string, ...params: string[] ): string => msg( props.data, key, ...params );

// The language's name in the interface language, where the browser knows it.
let languageName = props.data.lang;
try {
	languageName = new Intl.DisplayNames( [ props.data.lang ], { type: 'language' } ).of( props.data.lang ) || props.data.lang;
} catch {
	// An unknown code: keep it.
}

const label = ref( props.initial.label );
const description = ref( props.initial.description );
const aliases = ref( props.initial.aliases.join( '\n' ) );
const saving = ref( false );
const problem = ref( '' );
const conflict = ref( false );
const first = ref<{ focus:() => void } | null>( null );

onMounted( () => first.value?.focus() );

function cancel(): void {
	if ( !saving.value ) {
		emit( 'cancel' );
	}
}

async function save(): Promise<void> {
	const after: Terms = {
		label: label.value.trim(),
		description: description.value.trim(),
		aliases: aliases.value.split( '\n' ).map( ( a ) => a.trim() ).filter( ( a ) => a !== '' )
	};
	saving.value = true;
	problem.value = '';
	conflict.value = false;
	try {
		await props.api.write( {
			action: 'wbeditentity',
			id: props.data.id,
			baserevid: props.lastrevid,
			data: JSON.stringify( termsEdit( props.data.lang, after ) )
		} );
		emit( 'saved', after );
	} catch ( e ) {
		if ( e instanceof ApiError && e.code === 'editconflict' ) {
			conflict.value = true;
			problem.value = t( 'ts-edit-conflict' );
		} else {
			problem.value = t( 'ts-edit-failed', e instanceof Error ? e.message : String( e ) );
		}
	} finally {
		saving.value = false;
	}
}
</script>
